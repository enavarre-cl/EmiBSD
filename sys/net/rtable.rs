/*	$OpenBSD: rtable.h,v 1.36 2025/07/15 09:55:49 dlg Exp $ */
/*	$OpenBSD: rtable.c,v 1.95 2025/07/16 13:48:38 jsg Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2014-2016 Martin Pieuchot
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! The routing tables: `<net/rtable.h>` and `net/rtable.c`, the routing table backend over the
//! ART (`net/art.rs`).
//!
//! Upstream: sys/net/rtable.h @ 3ce1f3f79392
//! Upstream: sys/net/rtable.c @ 3ce1f3f79392
//!
//! `rtable_get` finds the table of a pair of address family and routing table id through
//! `afmap`: `afmap[af2idx[af]]` is an array of tables indexed by the id, and `afmap[0]` maps
//! every id to its routing domain and loopback interface (8 unused bits, 16 bits of loopback
//! index, 8 bits of rdomain). Table heads are never freed, so they are not reference
//! counted. Each ART node holds a list of `rtentry`s (`rt_next`) for the same destination and
//! prefix length, ordered by priority: the multipath routes.
//!
//! Locks, as in C: \[I\] immutable after creation, \[N\] net lock; each table's `r_lock`
//! serialises changes to its tree.
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - SRP (`kern/kern_srp.c`) is not ported. `afmap`'s slots are atomics read without a
//!   reference (`srp_enter`/`srp_leave` around a read that cannot sleep) and replaced under
//!   the kernel lock, as in C; `srp_update_locked`'s deferred `rtmap_dtor` is an `smr_call`
//!   through an [`SmrEntry`] in the old map (`rtm_smr`, which `struct rtmap` does not have),
//!   so the map is freed once every CPU has left the reads that may still see it, as the
//!   SRP garbage collector waits for its references. The lookups run inside SMR read
//!   sections around the ART (`net/art.rs`) and `rt_next` (`net/route.rs`), as in C.
//! - `struct rtmap` and `struct dommp` have the same layout in C (`rtable_init` asserts it)
//!   and `afmap[0]` is a `rtmap` read as a `dommp`; here both are [`Rtmap`], an array of words
//!   that hold a table pointer or a domain value.
//! - The key of a socket address (`satoaddr`) is a byte slice of the tree's address length.
//! - `rtable_satoplen` returns `Err(EINVAL)` for the C's -1 (a family without a routing table
//!   or a non-contiguous mask) and the prefix length otherwise.
//! - `rtable_walk`'s and `rtable_read`'s callback and its `void *arg` are a closure; its error
//!   is a `Result`. The `struct rtentry **prt` out parameter is an `Option<&mut ...>`.
//! - The functions that read socket addresses (`rtable_lookup`, `rtable_match`,
//!   `rtable_insert`, `rtable_delete`, `rtable_mpath_reprio`, `rtable_clearsource`,
//!   `rtable_satoplen`) are `unsafe fn`s: the addresses are raw pointers of any flavour
//!   (`docs/C_TO_RUST.md`).
//! - `malloc(M_WAITOK)` that fails panics (`malloc(9)` does not sleep yet).

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;
use core::slice;
use core::sync::atomic::{AtomicPtr, AtomicU8, AtomicU32, AtomicUsize, Ordering};

use crate::kassert;
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write, rw_init};
use crate::kern::kern_smr::{smr_read_enter, smr_read_leave};
use crate::kern::kern_synch::refcnt_read;
use crate::kern::subr_prf::panic;
use crate::kern::uipc_domain::DOMAINS;
use crate::net::art::{
    Art, ArtIter, ArtNode, art_alloc, art_boot, art_delete, art_get, art_insert, art_is_empty,
    art_iter_close, art_iter_next, art_iter_open, art_lookup, art_match, art_put,
};
use crate::net::route::{
    RTF_MPATH, RTP_ANY, RTP_MASK, Rtentry, rt_hash, rt_timer_init, rtfree, rtref,
};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_NOWAIT, M_RTABLE, M_WAITOK, M_ZERO};
use crate::sys::rwlock::Rwlock;
use crate::sys::smr::{SmrEntry, smr_call};
use crate::sys::socket::{AF_MAX, RT_TABLEID_BITS, RT_TABLEID_MASK, RT_TABLEID_MAX, Sockaddr};
use crate::sys::systm::{
    kernel_assert_locked, kernel_lock, kernel_unlock, net_assert_locked,
    net_assert_locked_exclusive,
};
use crate::sys::types::SaFamily;

/// `struct rtable`: a routing table of one address family.
pub struct Rtable {
    /// `r_lock`.
    pub r_lock: Rwlock,
    /// \[I\] `r_art`.
    pub r_art: &'static Art,
    /// \[I\] `r_off`: offset of key in bytes.
    pub r_off: u32,
    /// \[N\] `r_source`: use optional src addr.
    pub r_source: Cell<*const Sockaddr>,
}

// SAFETY: `r_source` changes under the exclusive net lock and the tree under `r_lock`, as the
// C's locking comment says; the other members are immutable.
unsafe impl Sync for Rtable {}

/// `struct rtmap` / `struct dommp`: an array of routing table pointers, or (for `afmap[0]`)
/// of rdomain/loopback values, `limit` words long (see the module's deviations).
pub struct Rtmap {
    /// `limit`.
    pub limit: u32,
    /// `tbl` / `value`: `limit` words.
    tbl: *const AtomicUsize,
    /// The deferred `rtmap_dtor` once the map is replaced (see the module's deviations).
    rtm_smr: SmrEntry,
}

impl Rtmap {
    /// `map->tbl[i]` / `dmm->value[i]`, for `i < limit`.
    fn slot(&self, i: u32) -> &AtomicUsize {
        if i >= self.limit {
            panic(format_args!("rtmap: index {i} of {}", self.limit));
        }
        // SAFETY: `rtmap_grow` allocated `limit` words at `tbl`, which live as long as the
        // map.
        unsafe { &*self.tbl.add(i as usize) }
    }
}

/// `afmap`: `af2idx_max + 1` slots, each the current map of one family (`afmap[0]`: the
/// rdomain/loopback values).
static AFMAP: AtomicPtr<AtomicPtr<Rtmap>> = AtomicPtr::new(ptr::null_mut());
/// `af2idx[]`: to only allocate supported AF.
static AF2IDX: [AtomicU8; AF_MAX as usize + 1] = [const { AtomicU8::new(0) }; AF_MAX as usize + 1];
/// `af2idx_max`.
static AF2IDX_MAX: AtomicU8 = AtomicU8::new(0);

/// `rtmap_limit`: the number of routing table ids.
pub static RTMAP_LIMIT: AtomicU32 = AtomicU32::new(0);

/// `rt_key(rt)`: the destination of a route.
pub fn rt_key(rt: &Rtentry) -> *mut Sockaddr {
    rt.rt_dest.get()
}

/// `rt_plen(rt)`: the prefix length of a route.
pub fn rt_plen(rt: &Rtentry) -> i32 {
    rt.rt_plen.get()
}

/// `RT_ROOT(rt)`: the ART has no root routes.
pub const fn rt_root(_rt: &Rtentry) -> bool {
    false
}

/// `afmap[idx]`.
fn afmap(idx: u8) -> &'static AtomicPtr<Rtmap> {
    let base = AFMAP.load(Ordering::Relaxed);
    if base.is_null() || idx > AF2IDX_MAX.load(Ordering::Relaxed) {
        panic(format_args!("afmap: index {idx}"));
    }
    // SAFETY: `rtable_init` allocated `af2idx_max + 1` slots that are never freed.
    unsafe { &*base.add(usize::from(idx)) }
}

/// The current map of `afmap[idx]`, NULL before its first `rtmap_grow`.
fn afmap_get(idx: u8) -> Option<&'static Rtmap> {
    // SAFETY: a map in `afmap` was written whole by `rtmap_grow` before it was published
    // (the `Release` store pairs with this `Acquire` load), and is freed by `smr_call` only
    // after it was replaced and every CPU left its reads (see the module's deviations).
    unsafe { afmap(idx).load(Ordering::Acquire).as_ref() }
}

/// `rtmap_init`.
fn rtmap_init() {
    // Start with a single table for every domain that requires it.
    for dp in DOMAINS {
        if dp.dom_rtoffset == 0 {
            continue;
        }

        rtmap_grow(1, dp.dom_family as SaFamily);
    }

    // Initialize the rtableid->rdomain mapping table.
    rtmap_grow(1, 0);

    RTMAP_LIMIT.store(1, Ordering::Relaxed);
}

/// `rtmap_grow`: grows the array of routing tables for `af` to `nlimit` entries.
fn rtmap_grow(nlimit: u32, af: SaFamily) {
    kernel_assert_locked();

    kassert!(nlimit > RTMAP_LIMIT.load(Ordering::Relaxed));

    let Some(nmap) = malloc(size_of::<Rtmap>(), M_RTABLE, M_WAITOK) else {
        panic(format_args!("rtmap_grow: no memory"));
    };
    let Some(tbl) = mallocarray(
        nlimit as usize,
        size_of::<AtomicUsize>(),
        M_RTABLE,
        M_WAITOK | M_ZERO,
    ) else {
        panic(format_args!("rtmap_grow: no memory"));
    };
    let nmap = nmap.cast::<Rtmap>().as_ptr();
    // SAFETY: a fresh block of `size_of::<Rtmap>()` bytes; the words are zeroed, which is a
    // valid `AtomicUsize` each.
    unsafe {
        nmap.write(Rtmap {
            limit: nlimit,
            tbl: tbl.cast::<AtomicUsize>().as_ptr(),
            rtm_smr: SmrEntry::new(),
        })
    };
    // SAFETY: just written.
    let nmap_ref = unsafe { &*nmap };

    let slot = afmap(AF2IDX[usize::from(af)].load(Ordering::Relaxed));
    let map = slot.load(Ordering::Relaxed);
    // SAFETY: the current map, alive until replaced below.
    if let Some(m) = unsafe { map.as_ref() } {
        kassert!(m.limit == RTMAP_LIMIT.load(Ordering::Relaxed));

        for i in 0..m.limit {
            nmap_ref
                .slot(i)
                .store(m.slot(i).load(Ordering::Relaxed), Ordering::Relaxed);
        }
    }

    // srp_update_locked(&rtmap_gc, ...): publish the new map, free the old one once no CPU
    // can still be reading it.
    slot.store(nmap, Ordering::Release);
    // SAFETY: the old map is out of `afmap`; it lives until `rtmap_dtor` runs.
    if let Some(m) = unsafe { map.as_ref() } {
        // SAFETY: as above: the entry lives in the map, which only the deferred call frees.
        let smr: &'static SmrEntry = unsafe { &*ptr::from_ref(&m.rtm_smr) };
        smr_call(smr, rtmap_dtor, map.cast());
    }
}

/// `rtmap_dtor`: frees a map that is no longer in `afmap`.
fn rtmap_dtor(xmap: *mut c_void) {
    let xmap = xmap.cast::<Rtmap>();
    // doesn't need to be serialized since this is the last reference to this map. there's
    // nothing to race against.
    // SAFETY: `xmap` was allocated by `rtmap_grow` and has just left `afmap`.
    let (limit, tbl) = unsafe { ((*xmap).limit, (*xmap).tbl) };
    if let Some(tbl) = ptr::NonNull::new(tbl.cast_mut()) {
        free(
            tbl.cast(),
            M_RTABLE,
            limit as usize * size_of::<AtomicUsize>(),
        );
    }
    if let Some(map) = ptr::NonNull::new(xmap) {
        free(map.cast(), M_RTABLE, size_of::<Rtmap>());
    }
}

/// `rtable_init`: the maps, the backend and the default routing table 0.
pub fn rtable_init() {
    // KASSERT(sizeof(struct rtmap) == sizeof(struct dommp)): one type here.

    // We use index 0 for the rtable/rdomain map.
    AF2IDX_MAX.store(1, Ordering::Relaxed);
    for a in &AF2IDX {
        a.store(0, Ordering::Relaxed);
    }

    // Compute the maximum supported key length in case the routing table backend needs it.
    for dp in DOMAINS {
        if dp.dom_rtoffset == 0 {
            continue;
        }

        let idx = AF2IDX_MAX.fetch_add(1, Ordering::Relaxed);
        AF2IDX[dp.dom_family as usize].store(idx, Ordering::Relaxed);
    }
    rtable_init_backend();

    // Allocate AF-to-id table now that we now how many AFs this kernel supports.
    let n = usize::from(AF2IDX_MAX.load(Ordering::Relaxed)) + 1;
    let Some(afmap) = mallocarray(
        n,
        size_of::<AtomicPtr<Rtmap>>(),
        M_RTABLE,
        M_WAITOK | M_ZERO,
    ) else {
        panic(format_args!("rtable_init: no memory"));
    };
    // Zeroed words are NULL `AtomicPtr`s.
    AFMAP.store(afmap.cast().as_ptr(), Ordering::Relaxed);

    rtmap_init();

    if rtable_add(0).is_err() {
        panic(format_args!("unable to create default routing table"));
    }

    rt_timer_init();
}

/// `rtable_add`: creates routing table `id` in every family that routes.
pub fn rtable_add(id: u32) -> Result<(), Errno> {
    if id > RT_TABLEID_MAX {
        return Err(Errno::EINVAL);
    }

    kernel_lock();

    let error = 'out: {
        if rtable_exists(id) {
            break 'out Ok(());
        }

        for dp in DOMAINS {
            if dp.dom_rtoffset == 0 {
                continue;
            }

            let af = dp.dom_family as SaFamily;
            let off = dp.dom_rtoffset;
            let alen = dp.dom_maxplen;

            if id >= RTMAP_LIMIT.load(Ordering::Relaxed) {
                rtmap_grow(id + 1, af);
            }

            let Some(tbl) = rtable_alloc(id, alen, off) else {
                break 'out Err(Errno::ENOMEM);
            };

            if let Some(map) = afmap_get(AF2IDX[usize::from(af)].load(Ordering::Relaxed)) {
                map.slot(id)
                    .store(ptr::from_ref(tbl) as usize, Ordering::Release);
            }
        }

        // Reflect possible growth.
        if id >= RTMAP_LIMIT.load(Ordering::Relaxed) {
            rtmap_grow(id + 1, 0);
            RTMAP_LIMIT.store(id + 1, Ordering::Relaxed);
        }

        // Use main rtable/rdomain by default.
        if let Some(dmm) = afmap_get(0) {
            dmm.slot(id).store(0, Ordering::Relaxed);
        }

        Ok(())
    };
    kernel_unlock();

    error
}

/// `rtable_get`: the table of `af` with id `rtableid`.
pub fn rtable_get(rtableid: u32, af: SaFamily) -> Option<&'static Rtable> {
    let idx = AF2IDX.get(usize::from(af))?.load(Ordering::Relaxed);
    if idx == 0 {
        return None;
    }

    let map = afmap_get(idx)?;
    if rtableid >= map.limit {
        return None;
    }
    // SAFETY: a non-zero slot holds a table `rtable_alloc` made, written whole before
    // `rtable_add` published it (`Release`, paired with this `Acquire`); tables are never
    // freed.
    unsafe { (map.slot(rtableid).load(Ordering::Acquire) as *const Rtable).as_ref() }
}

/// `rtable_exists`.
pub fn rtable_exists(rtableid: u32) -> bool {
    DOMAINS
        .iter()
        .filter(|dp| dp.dom_rtoffset != 0)
        .any(|dp| rtable_get(rtableid, dp.dom_family as SaFamily).is_some())
}

/// `rtable_empty`: whether no family's table `rtableid` holds a route.
pub fn rtable_empty(rtableid: u32) -> bool {
    for dp in DOMAINS {
        if dp.dom_rtoffset == 0 {
            continue;
        }

        let Some(tbl) = rtable_get(rtableid, dp.dom_family as SaFamily) else {
            continue;
        };
        if !art_is_empty(tbl.r_art) {
            return false;
        }
    }

    true
}

/// `rtable_l2`: the routing domain of table `rtableid` (0 for an unknown table).
pub fn rtable_l2(rtableid: u32) -> u32 {
    let mut rdomain = 0;

    if let Some(dmm) = afmap_get(0)
        && rtableid < dmm.limit
    {
        rdomain = dmm.slot(rtableid).load(Ordering::Relaxed) as u32 & RT_TABLEID_MASK;
    }

    rdomain
}

/// `rtable_loindex`: the loopback interface of table `rtableid` (0 when none).
pub fn rtable_loindex(rtableid: u32) -> u32 {
    let mut loifidx = 0;

    if let Some(dmm) = afmap_get(0)
        && rtableid < dmm.limit
    {
        loifidx = dmm.slot(rtableid).load(Ordering::Relaxed) as u32 >> RT_TABLEID_BITS;
    }

    loifidx
}

/// `rtable_l2set`: puts table `rtableid` in routing domain `rdomain` with loopback
/// interface `loifidx`.
pub fn rtable_l2set(rtableid: u32, rdomain: u32, loifidx: u32) {
    kernel_assert_locked();

    if !rtable_exists(rtableid) || !rtable_exists(rdomain) {
        return;
    }

    let value = (rdomain & RT_TABLEID_MASK) | (loifidx << RT_TABLEID_BITS);

    if let Some(dmm) = afmap_get(0) {
        dmm.slot(rtableid).store(value as usize, Ordering::Relaxed);
    }
}

/// `rtable_init_backend`.
fn rtable_init_backend() {
    art_boot();
}

/// `rtable_alloc`: a table for addresses of `alen` bits at offset `off` of the socket
/// address.
fn rtable_alloc(_rtableid: u32, alen: u32, off: u32) -> Option<&'static Rtable> {
    let tbl = malloc(size_of::<Rtable>(), M_RTABLE, M_NOWAIT | M_ZERO)?;

    let Some(art) = art_alloc(alen) else {
        free(tbl, M_RTABLE, size_of::<Rtable>());
        return None;
    };

    let tbl = tbl.cast::<Rtable>().as_ptr();
    // SAFETY: a fresh block of `size_of::<Rtable>()` bytes, written whole; tables are never
    // freed.
    let tbl = unsafe {
        tbl.write(Rtable {
            r_lock: Rwlock::new("rtable"),
            r_art: art,
            r_off: off,
            r_source: Cell::new(ptr::null()),
        });
        &*tbl
    };
    rw_init(&tbl.r_lock, "rtable");

    Some(tbl)
}

/// `rtable_setsource`: the preferred source address of `af` in table `rtableid`.
pub fn rtable_setsource(rtableid: u32, af: SaFamily, src: *const Sockaddr) -> Result<(), Errno> {
    net_assert_locked_exclusive("rtable_setsource");

    let Some(tbl) = rtable_get(rtableid, af) else {
        return Err(Errno::EAFNOSUPPORT);
    };

    tbl.r_source.set(src);

    Ok(())
}

/// `rtable_getsource`: the preferred source address of `af` in table `rtableid`, NULL if
/// none.
pub fn rtable_getsource(rtableid: u32, af: SaFamily) -> *const Sockaddr {
    net_assert_locked("rtable_getsource");

    match rtable_get(rtableid, af) {
        Some(tbl) => tbl.r_source.get(),
        None => ptr::null(),
    }
}

/// `rtable_clearsource`: forgets `src` as the preferred source address.
///
/// # Safety
///
/// `src` points at a readable socket address of its `sa_len` bytes.
pub unsafe fn rtable_clearsource(rtableid: u32, src: *const Sockaddr) {
    // SAFETY: the caller's contract.
    let (family, len) = unsafe { ((*src).sa_family, usize::from((*src).sa_len)) };
    let addr = rtable_getsource(rtableid, family);
    if addr.is_null() {
        return;
    }
    // SAFETY: a preferred source is an interface address, readable for its `sa_len`; `src`
    // is the caller's.
    let same = unsafe {
        usize::from((*addr).sa_len) == len
            && slice::from_raw_parts(src.cast::<u8>(), len)
                == slice::from_raw_parts(addr.cast::<u8>(), len)
    };
    if same {
        let _ = rtable_setsource(rtableid, family, ptr::null());
    }
}

/// The first route of a node's list (`an->an_value`).
fn an_rt(an: &ArtNode) -> Option<&'static Rtentry> {
    // SAFETY: a routing table's nodes hold NULL or a route the table has a reference on
    // (`rtable_insert`), which keeps it alive while it is listed.
    unsafe { (an.an_value.get() as *const Rtentry).as_ref() }
}

/// A link of a node's route list: the node's head or a route's `rt_next` (the C walks both
/// through one `struct rtentry **`).
#[derive(Clone, Copy)]
enum RtLink<'a> {
    Head(&'a ArtNode),
    Next(&'a Rtentry),
}

impl RtLink<'_> {
    fn get(self) -> Option<&'static Rtentry> {
        match self {
            RtLink::Head(an) => an_rt(an),
            RtLink::Next(rt) => rt.rt_next.get(),
        }
    }

    fn set(self, rt: Option<&'static Rtentry>) {
        match self {
            RtLink::Head(an) => an.an_value.set_locked(rt.map_or(ptr::null_mut(), |r| {
                ptr::from_ref(r).cast_mut().cast::<c_void>()
            })),
            RtLink::Next(prev) => prev.rt_next.set(rt),
        }
    }
}

/// `rtable_lookup`: the route of `dst`/`mask` (best match if `mask` is NULL) with gateway
/// `gateway` (any if NULL) and priority `prio` (any with `RTP_ANY`), referenced.
///
/// # Safety
///
/// `dst`, and `mask`/`gateway` when not NULL, point at readable socket addresses of their
/// `sa_len` bytes; `dst` holds at least the family's key.
pub unsafe fn rtable_lookup(
    rtableid: u32,
    dst: *const Sockaddr,
    mask: *const Sockaddr,
    gateway: *const Sockaddr,
    prio: u8,
) -> Option<&'static Rtentry> {
    // SAFETY: the caller's contract.
    let tbl = rtable_get(rtableid, unsafe { (*dst).sa_family })?;

    // SAFETY: the caller's contract.
    let addr = unsafe { satoaddr(tbl, dst) };

    smr_read_enter();
    let rt = (|| {
        let an = if mask.is_null() {
            // No need for a perfect match.
            art_match(tbl.r_art, addr)
        } else {
            // SAFETY: the caller's contract.
            let plen = unsafe { rtable_satoplen((*dst).sa_family, mask) }.ok()?;
            art_lookup(tbl.r_art, addr, plen)
        }?;

        let mut rt = an_rt(an);
        while let Some(r) = rt {
            if prio != RTP_ANY && (r.rt_priority.get() & RTP_MASK) != (prio & RTP_MASK) {
                rt = r.rt_next.get();
                continue;
            }

            // SAFETY: the caller's contract for `gateway`; a route's gateway is `sa_len`
            // bytes.
            if gateway.is_null() || unsafe { sa_equal(r.rt_gateway.get(), gateway) } {
                break;
            }
            rt = r.rt_next.get();
        }
        if let Some(r) = rt {
            rtref(r);
        }
        rt
    })();
    smr_read_leave();

    rt
}

/// `rtable_match`: the best route for `dst`, with Hash-Threshold gateway selection among
/// multipath routes when `src` is given; referenced.
///
/// # Safety
///
/// As for [`rtable_lookup`]; `src` holds the source address words `rt_hash` reads (one for
/// IPv4).
pub unsafe fn rtable_match(
    rtableid: u32,
    dst: *const Sockaddr,
    src: Option<&[u32]>,
) -> Option<&'static Rtentry> {
    // SAFETY: the caller's contract.
    let tbl = rtable_get(rtableid, unsafe { (*dst).sa_family })?;

    // SAFETY: the caller's contract.
    let addr = unsafe { satoaddr(tbl, dst) };

    smr_read_enter();
    // SAFETY: the caller's contract.
    let rt = unsafe { rtable_match_smr(tbl, dst, addr, src) };
    smr_read_leave();
    rt
}

/// The part of [`rtable_match`] inside its SMR read section.
///
/// # Safety
///
/// As for [`rtable_match`].
unsafe fn rtable_match_smr(
    tbl: &Rtable,
    dst: *const Sockaddr,
    addr: &[u8],
    src: Option<&[u32]>,
) -> Option<&'static Rtentry> {
    let an = art_match(tbl.r_art, addr)?;

    let Some(mut rt) = an_rt(an) else {
        panic(format_args!("rtable_match: node without routes"));
    };
    let prio = rt.rt_priority.get();

    // Gateway selection by Hash-Threshold (RFC 2992)
    // SAFETY: the caller's contract.
    if let Some(mut hash) = unsafe { rt_hash(rt, dst, src) } {
        kassert!(hash <= 0xffff);

        // Only count nexthops with the same priority.
        let mut npaths = 1;
        let mut mrt = rt.rt_next.get();
        while let Some(m) = mrt {
            if m.rt_priority.get() == prio {
                npaths += 1;
            }
            mrt = m.rt_next.get();
        }

        let threshold = (0xffff / npaths) + 1;

        // we have no protection against concurrent modification of the route list attached
        // to the node, so we won't necessarily have the same number of routes. for most
        // modifications, we'll pick a route that we wouldn't have if we only saw the list
        // before or after the change.
        let mut mrt = rt;
        while hash > threshold {
            if mrt.rt_priority.get() == prio {
                rt = mrt;
                hash -= threshold;
            }
            match mrt.rt_next.get() {
                Some(m) => mrt = m,
                None => break,
            }
        }
    }
    rtref(rt);
    Some(rt)
}

/// `rtable_insert`: inserts route `rt` for `dst`/`mask` with `gateway` and priority `prio`;
/// on success the table holds a reference on `rt`. `EEXIST` for the same destination, mask
/// and gateway (or any route with the same priority unless `rt` is `RTF_MPATH`).
///
/// # Safety
///
/// `dst` (which becomes the route's key) and `gateway` point at readable socket addresses of
/// their `sa_len` bytes, `mask` too unless NULL; `dst` lives as long as the route.
pub unsafe fn rtable_insert(
    rtableid: u32,
    dst: *mut Sockaddr,
    mask: *const Sockaddr,
    gateway: *const Sockaddr,
    prio: u8,
    rt: &'static Rtentry,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let family = unsafe { (*dst).sa_family };
    let Some(tbl) = rtable_get(rtableid, family) else {
        return Err(Errno::EAFNOSUPPORT);
    };

    // SAFETY: the caller's contract.
    let addr = unsafe { satoaddr(tbl, dst) };
    // SAFETY: the caller's contract.
    let plen = unsafe { rtable_satoplen(family, mask) }?;

    let Some(an) = art_get(addr, plen) else {
        return Err(Errno::ENOMEM);
    };

    // prepare for immediate operation if insert succeeds
    let rt_flags = rt.rt_flags.get();
    rt.rt_flags.set(rt.rt_flags.get() & !RTF_MPATH);
    rt.rt_dest.set(dst);
    rt.rt_plen.set(plen as i32);
    rt.rt_next.set(None);

    rtref(rt); // take a ref for the table
    RtLink::Head(an).set(Some(rt));

    rw_enter_write(&tbl.r_lock);
    let error = 'leave: {
        let Some(prev) = art_insert(tbl.r_art, an) else {
            art_put(an);
            break 'leave Err(Errno::ENOMEM);
        };

        if !ptr::eq(prev, an) {
            let mpathok = rt_flags & RTF_MPATH != 0;
            let mut mpath = 0;

            // An ART node with the same destination/netmask already exists.
            art_put(an);
            let an = prev;

            // Do not permit exactly the same dst/mask/gw pair.
            let mut mrt = an_rt(an);
            while let Some(m) = mrt {
                mrt = m.rt_next.get();
                if prio != RTP_ANY && (m.rt_priority.get() & RTP_MASK) != (prio & RTP_MASK) {
                    continue;
                }

                // SAFETY: the caller's contract for `gateway`; a route's gateway is `sa_len`
                // bytes.
                if !mpathok || unsafe { sa_equal(m.rt_gateway.get(), gateway) } {
                    break 'leave Err(Errno::EEXIST);
                }
                mpath = RTF_MPATH;
            }

            // The new route can be added to the list.
            if mpath != 0 {
                rt.rt_flags.set(rt.rt_flags.get() | RTF_MPATH);

                let mut mrt = an_rt(an);
                while let Some(m) = mrt {
                    if (m.rt_priority.get() & RTP_MASK) == (prio & RTP_MASK) {
                        m.rt_flags.set(m.rt_flags.get() | RTF_MPATH);
                    }
                    mrt = m.rt_next.get();
                }
            }

            // Put newly inserted entry at the right place.
            rtable_mpath_insert(an, rt);
        }
        rw_exit_write(&tbl.r_lock);
        return Ok(());
    };

    // put: / leave:
    rw_exit_write(&tbl.r_lock);
    rtfree(Some(rt));
    error
}

/// `rtable_delete`: removes route `rt` of `dst`/`mask` from the table and drops the table's
/// reference.
///
/// # Safety
///
/// As for [`rtable_lookup`].
pub unsafe fn rtable_delete(
    rtableid: u32,
    dst: *const Sockaddr,
    mask: *const Sockaddr,
    rt: &'static Rtentry,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let family = unsafe { (*dst).sa_family };
    let Some(tbl) = rtable_get(rtableid, family) else {
        return Err(Errno::EAFNOSUPPORT);
    };

    // SAFETY: the caller's contract.
    let addr = unsafe { satoaddr(tbl, dst) };
    // SAFETY: the caller's contract.
    let plen = unsafe { rtable_satoplen(family, mask) }?;

    rw_enter_write(&tbl.r_lock);
    smr_read_enter();
    let an = art_lookup(tbl.r_art, addr, plen);
    smr_read_leave();
    let Some(an) = an else {
        rw_exit_write(&tbl.r_lock);
        return Err(Errno::ESRCH);
    };

    // If this is the only route in the list then we can delete the node
    if an_rt(an).is_some_and(|first| ptr::eq(first, rt)) && rt.rt_next.get().is_none() {
        let oan = art_delete(tbl.r_art, addr, plen);
        if !oan.is_some_and(|o| ptr::eq(o, an)) {
            panic(format_args!(
                "art {:p} changed shape during delete",
                tbl.r_art
            ));
        }
        art_put(an);
        // XXX an and the rt ref could still be alive on other cpus. this currently works
        // because of the NET_LOCK/KERNEL_LOCK but should be fixed if we want to do route
        // lookups outside these locks. - dlg@
    } else {
        // If other multipath route entries are still attached to this ART node we only have
        // to unlink it.
        let mut found = false;
        let mut npaths = 0u32;
        let mut nrt: Option<&'static Rtentry> = None;

        let mut prt = RtLink::Head(an);
        while let Some(mrt) = prt.get() {
            if ptr::eq(mrt, rt) {
                found = true;
                prt.set(mrt.rt_next.get());
            } else if (mrt.rt_priority.get() & RTP_MASK) == (rt.rt_priority.get() & RTP_MASK) {
                npaths += 1;
                nrt = Some(mrt);
            }
            prt = RtLink::Next(mrt);
        }
        if !found {
            panic(format_args!("removing non-existent route"));
        }
        if npaths == 1
            && let Some(n) = nrt
        {
            n.rt_flags.set(n.rt_flags.get() & !RTF_MPATH);
        }
    }
    kassert!(refcnt_read(&rt.rt_refcnt) >= 1);
    rw_exit_write(&tbl.r_lock);
    rtfree(Some(rt));

    Ok(())
}

/// `rtable_walk`: calls `func` on every route of `af` in table `rtableid`, without the table
/// lock held. A `func` error stops the walk and is returned; the route it stopped at goes to
/// `prt` (referenced) when given.
pub fn rtable_walk(
    rtableid: u32,
    af: SaFamily,
    mut prt: Option<&mut Option<&'static Rtentry>>,
    mut func: impl FnMut(&'static Rtentry, u32) -> Result<(), Errno>,
) -> Result<(), Errno> {
    let Some(tbl) = rtable_get(rtableid, af) else {
        return Err(Errno::EAFNOSUPPORT);
    };

    let mut ai = ArtIter::new();
    rw_enter_write(&tbl.r_lock);
    let mut node = art_iter_open(tbl.r_art, &mut ai);
    while let Some(an) = node {
        // ART nodes have a list of rtentries.
        //
        // art_iter holds references to the topology so it won't change, but not the an_node
        // or rtentries.
        let Some(mut rt) = an_rt(an) else {
            panic(format_args!("rtable_walk: node without routes"));
        };
        rtref(rt);

        rw_exit_write(&tbl.r_lock);
        loop {
            smr_read_enter();
            // Get ready for the next entry.
            let nrt = rt.rt_next.get();
            if let Some(n) = nrt {
                rtref(n);
            }
            smr_read_leave();

            if let Err(error) = func(rt, rtableid) {
                match prt.as_deref_mut() {
                    Some(p) => *p = Some(rt),
                    None => rtfree(Some(rt)),
                }

                rtfree(nrt);

                rw_enter_write(&tbl.r_lock);
                art_iter_close(&mut ai);
                rw_exit_write(&tbl.r_lock);
                return Err(error);
            }

            rtfree(Some(rt));
            match nrt {
                Some(n) => rt = n,
                None => break,
            }
        }
        rw_enter_write(&tbl.r_lock);
        node = art_iter_next(&mut ai);
    }
    rw_exit_write(&tbl.r_lock);

    Ok(())
}

/// `rtable_read`: calls `func` on every route of `af` in table `rtableid` with the table
/// lock held; a `func` error stops the walk and is returned.
pub fn rtable_read(
    rtableid: u32,
    af: SaFamily,
    mut func: impl FnMut(&Rtentry, u32) -> Result<(), Errno>,
) -> Result<(), Errno> {
    let Some(tbl) = rtable_get(rtableid, af) else {
        return Err(Errno::EAFNOSUPPORT);
    };

    let mut error = Ok(());
    let mut ai = ArtIter::new();
    rw_enter_write(&tbl.r_lock);
    let mut node = art_iter_open(tbl.r_art, &mut ai);
    'leave: while let Some(an) = node {
        let mut rt = an_rt(an);
        while let Some(r) = rt {
            if let Err(e) = func(r, rtableid) {
                error = Err(e);
                art_iter_close(&mut ai);
                break 'leave;
            }
            rt = r.rt_next.get();
        }
        node = art_iter_next(&mut ai);
    }
    rw_exit_write(&tbl.r_lock);

    error
}

/// `rtable_iterate`: the next route of `rt0`'s multipath list, referenced; drops `rt0`.
pub fn rtable_iterate(rt0: &'static Rtentry) -> Option<&'static Rtentry> {
    smr_read_enter();
    let rt = rt0.rt_next.get();
    if let Some(r) = rt {
        rtref(r);
    }
    smr_read_leave();
    rtfree(Some(rt0));
    rt
}

/// `rtable_mpath_capable`: the ART backend always supports multipath.
pub fn rtable_mpath_capable(_rtableid: u32, _af: SaFamily) -> bool {
    true
}

/// `rtable_mpath_reprio`: changes the priority of route `rt` of `dst`/`plen` to `prio`,
/// moving it in its list. `EAGAIN` when it moved, `ESRCH` without such a node.
///
/// # Safety
///
/// As for [`rtable_lookup`].
pub unsafe fn rtable_mpath_reprio(
    rtableid: u32,
    dst: *const Sockaddr,
    plen: i32,
    prio: u8,
    rt: &'static Rtentry,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let Some(tbl) = rtable_get(rtableid, unsafe { (*dst).sa_family }) else {
        return Err(Errno::EAFNOSUPPORT);
    };

    // SAFETY: the caller's contract.
    let addr = unsafe { satoaddr(tbl, dst) };

    let mut error = Ok(());
    rw_enter_write(&tbl.r_lock);
    smr_read_enter();
    let an = art_lookup(tbl.r_art, addr, plen as u32);
    smr_read_leave();
    match an {
        None => error = Err(Errno::ESRCH),
        Some(an) if an_rt(an).is_some_and(|f| ptr::eq(f, rt)) && rt.rt_next.get().is_none() => {
            // If there's only one entry on the list do not go through an insert/remove cycle.
            // This is done to guarantee that ``an->an_rtlist'' is never empty when a node is
            // in the tree.
            rt.rt_priority.set(prio);
        }
        Some(an) => {
            let mut prt = RtLink::Head(an);
            let mut mrt = prt.get();
            while let Some(m) = mrt {
                if ptr::eq(m, rt) {
                    break;
                }
                prt = RtLink::Next(m);
                mrt = m.rt_next.get();
            }
            kassert!(mrt.is_some());

            prt.set(rt.rt_next.get());
            rt.rt_priority.set(prio);
            rtable_mpath_insert(an, rt);
            error = Err(Errno::EAGAIN);
        }
    }
    rw_exit_write(&tbl.r_lock);

    error
}

/// `rtable_mpath_insert`: links `rt` into `an`'s list before the first route of a higher
/// priority value.
fn rtable_mpath_insert(an: &ArtNode, rt: &'static Rtentry) {
    let prio = rt.rt_priority.get();

    // Iterate until we find the route to be placed after ``rt''.
    let mut prt = RtLink::Head(an);
    let mut mrt = prt.get();
    while let Some(m) = mrt {
        if m.rt_priority.get() > prio {
            break;
        }

        prt = RtLink::Next(m);
        mrt = m.rt_next.get();
    }

    rt.rt_next.set(mrt);
    prt.set(Some(rt));
}

/// `satoaddr`: the key of a socket address, skipping the non-address fields of its flavour
/// (an heritage from the BSD radix tree).
///
/// # Safety
///
/// `sa` points at a readable socket address at least `r_off` plus the key's bytes long, valid
/// for the returned lifetime.
unsafe fn satoaddr<'a>(tbl: &Rtable, sa: *const Sockaddr) -> &'a [u8] {
    let len = tbl.r_art.art_alen.get().div_ceil(8) as usize;
    // SAFETY: the caller's contract.
    unsafe { slice::from_raw_parts(sa.cast::<u8>().add(tbl.r_off as usize), len) }
}

/// `rtable_satoplen`: the prefix length of `mask` in family `af`; the family's maximum for a
/// NULL mask (a host route). `EINVAL` for the C's -1.
///
/// # Safety
///
/// `mask` is NULL or points at a readable socket address of its `sa_len` bytes.
pub unsafe fn rtable_satoplen(af: SaFamily, mask: *const Sockaddr) -> Result<u32, Errno> {
    let Some(dp) = DOMAINS
        .iter()
        .find(|dp| dp.dom_rtoffset != 0 && dp.dom_family == i32::from(af))
    else {
        return Err(Errno::EINVAL);
    };

    // Host route
    if mask.is_null() {
        return Ok(dp.dom_maxplen);
    }

    // SAFETY: the caller's contract.
    let mlen = usize::from(unsafe { (*mask).sa_len });

    // Default route
    if mlen == 0 {
        return Ok(0);
    }

    let off = dp.dom_rtoffset as usize;
    if off > mlen {
        return Err(Errno::EINVAL);
    }
    // SAFETY: the caller's contract: `mlen` readable bytes.
    let bytes = unsafe { slice::from_raw_parts(mask.cast::<u8>(), mlen) };
    let mut key = &bytes[off..];

    // Trim trailing zeroes.
    while let [rest @ .., 0] = key {
        key = rest;
    }

    if key.is_empty() {
        return Ok(0);
    }

    // "Beauty" adapted from sbin/route/show.c ...
    let mut plen = 0u32;
    let mut used = 0;
    for &b in key {
        used += 1;
        match b {
            0xff => plen += 8,
            0xfe | 0xfc | 0xf8 | 0xf0 | 0xe0 | 0xc0 | 0x80 => {
                plen += b.leading_ones();
                break;
            }
            // Non contiguous mask.
            _ => return Err(Errno::EINVAL),
        }
    }

    if plen > dp.dom_maxplen || used != key.len() {
        return Err(Errno::EINVAL);
    }

    Ok(plen)
}

/// Whether two socket addresses are the same `a->sa_len` bytes (and have the same length).
///
/// # Safety
///
/// Both point at readable socket addresses of their `sa_len` bytes.
pub unsafe fn sa_equal(a: *const Sockaddr, b: *const Sockaddr) -> bool {
    // SAFETY: the caller's contract.
    unsafe {
        let len = usize::from((*a).sa_len);
        usize::from((*b).sa_len) == len
            && slice::from_raw_parts(a.cast::<u8>(), len)
                == slice::from_raw_parts(b.cast::<u8>(), len)
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the routing tables: masks to prefix lengths, the table maps, and routes
    // inserted, matched, listed and deleted through an interface address.

    use std::vec::Vec;

    use super::*;
    use crate::netinet::in_::{SockaddrIn, sintosa};
    use crate::sys::socket::AF_INET;

    /// A `sockaddr_in` for `a` with length `len`.
    fn sin(a: [u8; 4], len: u8) -> SockaddrIn {
        SockaddrIn {
            sin_len: len,
            sin_family: AF_INET,
            sin_addr: crate::netinet::in_::InAddr {
                s_addr: u32::from_ne_bytes(a),
            },
            ..SockaddrIn::default()
        }
    }

    fn plen(mask: Option<SockaddrIn>) -> Result<u32, Errno> {
        match mask {
            // SAFETY: a local `sockaddr_in`.
            Some(mut m) => unsafe { rtable_satoplen(AF_INET, sintosa(&mut m)) },
            // SAFETY: a NULL mask is a host route.
            None => unsafe { rtable_satoplen(AF_INET, ptr::null()) },
        }
    }

    #[test]
    fn masks_become_prefix_lengths() {
        let _g = crate::netinet::ip_input::tests::setup();
        assert_eq!(plen(None), Ok(32), "host route");
        assert_eq!(plen(Some(sin([0; 4], 0))), Ok(0), "sa_len 0: default route");
        assert_eq!(plen(Some(sin([0; 4], 16))), Ok(0));
        assert_eq!(plen(Some(sin([255, 255, 255, 0], 16))), Ok(24));
        assert_eq!(
            plen(Some(sin([255, 255, 255, 0], 7))),
            Ok(24),
            "trimmed by in_socktrim"
        );
        assert_eq!(plen(Some(sin([255, 255, 254, 0], 16))), Ok(23));
        assert_eq!(plen(Some(sin([255, 128, 0, 0], 16))), Ok(9));
        assert_eq!(plen(Some(sin([255, 255, 255, 255], 16))), Ok(32));
        assert_eq!(
            plen(Some(sin([255, 0, 255, 0], 16))),
            Err(Errno::EINVAL),
            "non contiguous"
        );
        assert_eq!(plen(Some(sin([255, 0x7f, 0, 0], 16))), Err(Errno::EINVAL));
        // SAFETY: a NULL mask.
        assert_eq!(
            unsafe { rtable_satoplen(crate::sys::socket::AF_UNIX, ptr::null()) },
            Err(Errno::EINVAL)
        );
    }

    #[test]
    fn table_zero_exists_in_routing_domain_zero() {
        let _g = crate::netinet::ip_input::tests::setup();
        assert!(rtable_exists(0));
        assert!(!rtable_exists(1));
        assert!(rtable_get(0, AF_INET).is_some());
        assert!(rtable_get(0, crate::sys::socket::AF_UNIX).is_none());
        assert_eq!(rtable_l2(0), 0);

        // A new table, in rdomain 0 with lo0 until rtable_l2set.
        rtable_add(3).expect("rtable_add");
        assert!(rtable_exists(3) && !rtable_exists(2));
        assert!(rtable_empty(3));
        assert_eq!(rtable_l2(3), 0);
        rtable_l2set(3, 3, 9);
        assert_eq!(rtable_l2(3), 3);
        assert_eq!(rtable_loindex(3), 9);
        assert_eq!(rtable_add(RT_TABLEID_MAX + 1), Err(Errno::EINVAL));
    }

    #[test]
    fn routes_of_an_address_are_matched_listed_and_deleted() {
        let _g = crate::netinet::ip_input::tests::setup();
        let ifp = crate::netinet::ip_input::tests::test_ether();
        crate::netinet::ip_input::tests::configure(ifp, [10, 0, 2, 15], [255, 255, 255, 0]);

        // The routes in_ifinit made, by walking the table.
        let mut seen: Vec<(Vec<u8>, i32, u32)> = Vec::new();
        rtable_walk(0, AF_INET, None, |rt, _| {
            // SAFETY: a route's key is a `sockaddr_in` here.
            let key = unsafe { (*crate::netinet::in_::satosin_const(rt_key(rt))).sin_addr };
            seen.push((
                key.s_addr.to_ne_bytes().to_vec(),
                rt_plen(rt),
                rt.rt_flags.get(),
            ));
            Ok(())
        })
        .expect("walk");
        let has = |a: [u8; 4], plen: i32| seen.iter().any(|(k, p, _)| *k == a && *p == plen);
        assert!(has([10, 0, 2, 15], 32), "local route: {seen:?}");
        assert!(has([10, 0, 2, 0], 24), "prefix route: {seen:?}");
        assert!(has([10, 0, 2, 255], 32), "broadcast route: {seen:?}");

        // rtable_read stops at the first error.
        let mut n = 0;
        assert_eq!(
            rtable_read(0, AF_INET, |_, _| {
                n += 1;
                Err(Errno::EEXIST)
            }),
            Err(Errno::EEXIST)
        );
        assert_eq!(n, 1);

        // Longest prefix: an address of the subnet matches the cloning route, a perfect lookup of
        // the /24 finds it too, a /25 does not exist.
        let mut a = sin([10, 0, 2, 77], 16);
        // SAFETY: local `sockaddr_in`s.
        let rt = unsafe { rtable_match(0, sintosa(&mut a), None) }.expect("match");
        assert_eq!(rt_plen(rt), 24);
        crate::net::route::rtfree(Some(rt));
        let mut net = sin([10, 0, 2, 0], 16);
        let mut m24 = sin([255, 255, 255, 0], 16);
        let mut m25 = sin([255, 255, 255, 128], 16);
        // SAFETY: local `sockaddr_in`s.
        unsafe {
            let rt = rtable_lookup(
                0,
                sintosa(&mut net),
                sintosa(&mut m24),
                ptr::null(),
                RTP_ANY,
            )
            .expect("perfect lookup");
            crate::net::route::rtfree(Some(rt));
            assert!(
                rtable_lookup(
                    0,
                    sintosa(&mut net),
                    sintosa(&mut m25),
                    ptr::null(),
                    RTP_ANY
                )
                .is_none()
            );
        }

        // Removing the address empties the table.
        crate::netinet::ip_input::tests::unconfigure(ifp, [10, 0, 2, 15]);
        assert!(rtable_empty(0));
    }
}
/* </TESTS> */
