/*	$OpenBSD: pf_table.c,v 1.150 2026/09/21 13:58:20 gnezdo Exp $	*/
/* <LICENSES> */

/*
 * Copyright (c) 2002 Cedric Berger
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 *    - Redistributions of source code must retain the above copyright
 *      notice, this list of conditions and the following disclaimer.
 *    - Redistributions in binary form must reproduce the above
 *      copyright notice, this list of conditions and the following
 *      disclaimer in the documentation and/or other materials provided
 *      with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 * "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 * LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
 * FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
 * COPYRIGHT HOLDERS OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
 * BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 * CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
 * ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 *
 */
/* </LICENSES> */

//! pf's address tables (`pfctl -t`, `<table>` in rules): sets of IPv4 and IPv6 addresses and
//! networks, kept in radix trees for longest-prefix matches.
//!
//! Upstream: sys/net/pf_table.c @ 3ce1f3f79392
//!
//! A table (`struct pfr_ktable`) is an item of `pfr_ktable_pl` in the global tree
//! `pfr_ktables`, ordered by name then anchor; it holds two radix trees (`pfrkt_ip4`,
//! `pfrkt_ip6`) of entries (`struct pfr_kentry`, items of `pfr_kentry_pl[type]`). An entry
//! embeds the radix node pair it is linked by as its first member (`#[repr(C)]` in
//! `net/pfvar.rs`), so the leaf the radix code returns is the entry. Transactions build a
//! shadow table (`pfr_ina_define`) that `pfr_ina_commit` merges or swaps in; an anchored table
//! refers to the root table of the same name (`pfrkt_root`), which rules in the anchor fall back
//! to when the anchored table is not active.
//!
//! The ioctl functions take their arrays as a [`PfrBuf`]: the C's `COPYIN`/`COPYOUT` choose
//! `copyin`/`copyout` at a user address when the flags have `PFR_FLAG_USERIOCTL` and `bcopy` to
//! or from a kernel array otherwise; the buffer is the user address (`PfrBuf::User`) or the
//! kernel array (`PfrBuf::Kernel`) itself, and the flags still pick the path.
//!
//! ## Deviations
//! - `COPYIN`/`COPYOUT` through a [`PfrBuf`]: a buffer of the other kind than the flags ask for,
//!   or an index past the end of a kernel array, is `EFAULT` (the C would `bcopy` from the user
//!   address, or `copyin` from a kernel address, or run off the array).
//! - `pfr_walktree`'s `struct pfr_walktree` is [`PfrWalktree`], its union an enum; it is passed
//!   to `rn_walktree` in a closure. `PFRW_POOL_GET`'s "finish search" return of 1 is
//!   `Err(EPERM)` (`EPERM` is 1), which `pfr_kentry_byidx` ignores as the C does.
//! - `pfr_kentry_all`'s `pfrke_rkif`/`pfrke_rifname` are reached through `pfr_kentry_route`, and
//!   the cost entry's weight through `pfr_kentry_cost`, after the entry's type.
//!   `pfr_fill_feedback` reads the weight of every entry as a `pfr_kentry_cost`, past the end of
//!   plain and route entries; here only cost entries report their weight, the others 0.
//! - The `sockaddr_in` and `sockaddr_in6` keys built on the stack (`tmp4`/`tmp6` in
//!   `pfr_kentry_byaddr`, `pfr_update_stats`, `pfr_pool_get`) are `pfsockaddr_union`s, so that
//!   the radix code may read them for its whole key length (`sizeof(struct sockaddr_in6)`).
//!   `pfr_pool_get`'s `addr`, a `struct pf_addr *` into `tmp4.sin_addr` or `tmp6.sin6_addr`, is
//!   a local address written into the key before each lookup.
//! - `RB_FIND` with a `struct pfr_table` cast to a `struct pfr_ktable` (`pfr_lookup_table`,
//!   the `key` tables of `pfr_add_tables`, `pfr_del_tables`, `pfr_ina_define`, ...) is a
//!   descent of `pfr_ktables` comparing the table's name and anchor as `pfr_ktable_compare`
//!   does; the key tables are heap copies of a `struct pfr_table`.
//! - Out-pointers: `int *` counts are `Option<&mut i32>` (NULL is `None`), in-out sizes
//!   `&mut i32`; `pfr_pool_get`'s `raddr`/`rmask` (pointers into the entry and into
//!   `pfr_mask`) are returned as copies, `Err(1)` and `Err(-1)` being the C's 1 and -1.
//! - Functions whose C `int` is 0 or -1 but not an errno (`pfr_validate_addr`,
//!   `pfr_route_kentry`, `pfr_unroute_kentry`) return `bool`, `true` for the C's 0; the
//!   predicates `pfr_skip_table`, `pfr_islinklocal` and `pfr_match_addr` return `true` where
//!   the C returns nonzero. `pfr_insert_kentry` returns `pfr_route_kentry`'s -1 as the C does,
//!   which is `ERESTART`'s value; like the C, it does not free the entry it could not route.
//! - `pfr_insert_kentry` and `pfr_remove_kentry` take `&PfrAddr` (pf.c's callers); the C's
//!   `pfr_create_kentry` writes the implicit weight into the caller's `pfr_addr`, here into a
//!   copy.
//! - Where the C dereferences a pointer it takes to be set (a table's radix heads, the shadow of
//!   a table being committed, the entry `rn_match` finds inside a block), a NULL one panics.
//! - `pfr_kentry_pl` is an array of `PFRKE_MAX` pools, as in C; `pf_pool_limits` names its
//!   first pool (`&pfr_kentry_pl` in C is the array's address, `pfr_kentry_pl[PFRKE_PLAIN]`).

use core::cell::Cell;
use core::cmp::Ordering;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};

use crate::kern::kern_malloc::free;
use crate::kern::kern_tc::gettime;
use crate::kern::sched_bsd::preempt;
use crate::kern::subr_pool::pool_get;
use crate::kern::subr_prf::panic;
use crate::machine::copy::{copyin, copyout};
use crate::machine::intr::IPL_SOFTNET;
use crate::net::if_::unhandled_af;
use crate::net::pf::{pf_addr_inc, pf_addrcpy, pf_match_addr, pf_poolmask};
use crate::net::pf_if::{pfi_kif_get, pfi_kif_ref, pfi_kif_unref};
use crate::net::pf_ruleset::{
    pf_find_or_create_ruleset, pf_find_ruleset, pf_main_ruleset, pf_remove_if_empty_ruleset,
};
use crate::net::pfvar::{
    PF_ADDR_DYNIFTL, PF_ADDR_TABLE, PF_DROP, PF_MATCH, PF_OUT, PF_PASS, PF_RESERVED_ANCHOR,
    PF_TABLE_NAME_SIZE, PFI_KIF_REF_ROUTE, PFR_DIR_MAX, PFR_FB_ADDED, PFR_FB_CHANGED,
    PFR_FB_CLEARED, PFR_FB_CONFLICT, PFR_FB_DELETED, PFR_FB_DUPLICATE, PFR_FB_MATCH,
    PFR_FB_NOCOUNT, PFR_FB_NONE, PFR_FB_NOTMATCH, PFR_FLAG_ADDRSTOO, PFR_FLAG_ALLMASK,
    PFR_FLAG_ALLRSETS, PFR_FLAG_CLSTATS, PFR_FLAG_DUMMY, PFR_FLAG_FEEDBACK, PFR_FLAG_REPLACE,
    PFR_FLAG_USERIOCTL, PFR_OP_ADDR_MAX, PFR_OP_BLOCK, PFR_OP_MATCH, PFR_OP_PASS, PFR_OP_TABLE_MAX,
    PFR_OP_XPASS, PFR_REFCNT_ANCHOR, PFR_REFCNT_RULE, PFR_TFLAG_ACTIVE, PFR_TFLAG_CONST,
    PFR_TFLAG_COUNTERS, PFR_TFLAG_INACTIVE, PFR_TFLAG_PERSIST, PFR_TFLAG_REFDANCHOR,
    PFR_TFLAG_REFERENCED, PFR_TFLAG_SETMASK, PFR_TFLAG_USRMASK, PFRKE_COST, PFRKE_FLAG_MARK,
    PFRKE_FLAG_NOT, PFRKE_MAX, PFRKE_PLAIN, PFRKE_ROUTE, PfAddr, PfPool, PfPoolItem, PfRuleset,
    PfiDynaddr, PfrAddr, PfrAstats, PfrKcounters, PfrKentry, PfrKentryCost, PfrKentryRoute,
    PfrKentryioq, PfrKentryworkq, PfrKtable, PfrKtableworkq, PfrTable, PfrTstats, PfsockaddrUnion,
    pf_abi_zeroed, pf_azero, pf_cstr, pf_pool_get, pf_pool_init, pf_pool_put,
};
use crate::net::pfvar_priv::{PfGlobal, PfPdesc, pf_assert_locked, pf_lock, pf_unlock};
use crate::net::radix::{
    RadixNode, RadixNodeHead, rn_addroute, rn_delete, rn_init, rn_inithead, rn_lookup, rn_match,
    rn_walktree,
};
use crate::netinet::in_::{InAddr, SockaddrIn};
use crate::netinet6::in6::SockaddrIn6;
use crate::sys::errno::Errno;
use crate::sys::malloc::M_RTABLE;
use crate::sys::pool::{PR_LIMITFAIL, PR_NOWAIT, PR_WAITOK, PR_ZERO, Pool};
use crate::sys::queue::{SlistAdapter, SlistHead};
use crate::sys::sched::sched_pause;
use crate::sys::socket::{AF_INET, AF_UNSPEC};
use crate::sys::syslimits::PATH_MAX;
use crate::sys::syslog::{LOG_DEBUG, LOG_ERR, LOG_NOTICE};
use crate::sys::systm::{net_assert_locked, net_lock, net_unlock};
use crate::sys::tree::RbHead;
use crate::sys::types::{SaFamily, Time};
use libkern::{strlcpy, strnlen};

#[cfg(feature = "inet6")]
use crate::{netinet6::in6::In6Addr, sys::socket::AF_INET6};

/// `NO_ADDRESSES`: a shadow table's `pfrkt_cnt` when the transaction gave no addresses.
const NO_ADDRESSES: i32 = -1;
/// `ENQUEUE_UNMARKED_ONLY`.
const ENQUEUE_UNMARKED_ONLY: bool = true;
/// `INVERT_NEG_FLAG`.
const INVERT_NEG_FLAG: bool = true;

/// A `struct pfr_addr *`, `struct pfr_table *`, ... array argument: a user address
/// (`PFR_FLAG_USERIOCTL`, `copyin`/`copyout`) or a kernel array (`bcopy`).
pub enum PfrBuf<'a, T> {
    /// The array at this user address.
    User(usize),
    /// This kernel array.
    Kernel(&'a mut [T]),
}

/// `enum pfrw_op`: what `pfr_walktree` does with each entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PfrwOp {
    /// `PFRW_MARK`.
    Mark,
    /// `PFRW_SWEEP`.
    Sweep,
    /// `PFRW_ENQUEUE`.
    Enqueue,
    /// `PFRW_GET_ADDRS`.
    GetAddrs,
    /// `PFRW_GET_ASTATS`.
    GetAstats,
    /// `PFRW_POOL_GET`.
    PoolGet,
    /// `PFRW_DYNADDR_UPDATE`.
    DynaddrUpdate,
}

/// The union `pfrw_1` of `struct pfr_walktree`; the array pointers are a buffer and the index
/// of the next element.
enum Pfrw1<'a, 'b> {
    /// Nothing (`bzero`).
    None,
    /// `pfrw_addr`.
    Addr(&'a mut PfrBuf<'b, PfrAddr>, usize),
    /// `pfrw_astats`.
    Astats(&'a mut PfrBuf<'b, PfrAstats>, usize),
    /// `pfrw_workq`.
    Workq(&'a SlistHead<PfrKentryworkq>),
    /// `pfrw_kentry`.
    Kentry(Option<&'static PfrKentry>),
    /// `pfrw_dyn`.
    Dyn(&'static PfiDynaddr),
}

/// `struct pfr_walktree`: the argument of `pfr_walktree`.
struct PfrWalktree<'a, 'b> {
    /// `pfrw_op`.
    pfrw_op: PfrwOp,
    /// `pfrw_1`.
    pfrw_1: Pfrw1<'a, 'b>,
    /// `pfrw_free`, also `pfrw_cnt`.
    pfrw_free: i32,
    /// `pfrw_flags`.
    pfrw_flags: i32,
}

impl PfrWalktree<'_, '_> {
    /// `bzero(&w, sizeof(w)); w.pfrw_op = op;`.
    fn new(op: PfrwOp) -> Self {
        Self {
            pfrw_op: op,
            pfrw_1: Pfrw1::None,
            pfrw_free: 0,
            pfrw_flags: 0,
        }
    }
}

crate::tree_adapter!(
    /// `RB_HEAD(pfr_ktablehead, pfr_ktable)`, `RB_GENERATE(pfr_ktablehead, ...)`.
    pub PfrKtablehead: PfrKtable, pfrkt_tree => crate::sys::tree::RbEntry<PfrKtable>,
    pfr_ktable_compare
);

/// `pfr_ktable_pl`.
pub static PFR_KTABLE_PL: Pool = Pool::new();
/// `pfr_kentry_pl[PFRKE_MAX]`: plain, route and cost entries.
pub static PFR_KENTRY_PL: [Pool; PFRKE_MAX as usize] = [Pool::new(), Pool::new(), Pool::new()];
/// `pfr_kcounters_pl`.
pub static PFR_KCOUNTERS_PL: Pool = Pool::new();

// SAFETY: integers, `Cell`s of them and of `Option<&T>`, tree and list links and a byte copy
// of a `PfAbi` table: the all-zero pattern is valid.
unsafe impl PfPoolItem for PfrKtable {}
// SAFETY: radix nodes (null links and zeros, `RadixNode::new()`), list links, `Cell`s of
// integers, of a byte union and of `Option<&T>`: the all-zero pattern is valid.
unsafe impl PfPoolItem for PfrKentry {}
// SAFETY: as for `PfrKentry`, plus a `Cell<Option<&T>>` and a byte array.
unsafe impl PfPoolItem for PfrKentryRoute {}
// SAFETY: as for `PfrKentryRoute`, plus a `Cell<u16>`.
unsafe impl PfPoolItem for PfrKentryCost {}
// SAFETY: `Cell`s of integers.
unsafe impl PfPoolItem for PfrKcounters {}

/// `pfr_mask`: the mask `pfr_pool_get` hands out.
pub static PFR_MASK: PfGlobal<Cell<PfsockaddrUnion>> =
    PfGlobal(Cell::new(PfsockaddrUnion { bytes: [0; 28] }));
/// `pfr_ffaddr`: all ones, set by `pfr_initialize`.
pub static PFR_FFADDR: PfGlobal<Cell<PfAddr>> = PfGlobal(Cell::new(PfAddr::zeroed()));

/// `pfr_ktables`: every table, by name and anchor.
pub static PFR_KTABLES: PfGlobal<RbHead<PfrKtablehead>> = PfGlobal(RbHead::new());
/// `pfr_nulltable`: the empty description of the temporary tables.
pub static PFR_NULLTABLE: PfGlobal<PfrTable> = PfGlobal(PfrTable::zeroed());
/// `pfr_ktable_cnt`: the number of tables in `pfr_ktables`.
pub static PFR_KTABLE_CNT: PfGlobal<Cell<i32>> = PfGlobal(Cell::new(0));

/// An element type of a [`PfrBuf`]: moved as its bytes.
///
/// # Safety
///
/// Implement only for `#[repr(C)]` types without implicit padding whose every bit pattern is
/// valid (the promise of `AbiPod` and `PfAbi`).
pub unsafe trait PfrIo: Sized {}

// SAFETY: `PfrAddr: AbiPod`.
unsafe impl PfrIo for PfrAddr {}
// SAFETY: `PfrAstats: AbiPod`.
unsafe impl PfrIo for PfrAstats {}
// SAFETY: `PfrTable: PfAbi`.
unsafe impl PfrIo for PfrTable {}
// SAFETY: `PfrTstats: PfAbi`.
unsafe impl PfrIo for PfrTstats {}

/// `ACCEPT_FLAGS(flags, oklist)`: `EINVAL` for a flag outside `oklist`.
fn accept_flags(flags: i32, oklist: i32) -> Result<(), Errno> {
    if (flags & !oklist) & PFR_FLAG_ALLMASK != 0 {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

/// The bytes of `v`.
fn io_bytes<T: PfrIo>(v: &T) -> &[u8] {
    // SAFETY: `T: PfrIo` has no implicit padding, so every byte is initialised; nothing
    // changes `v` while the slice is borrowed (pf's locks, or the caller owns `v`).
    unsafe { core::slice::from_raw_parts(ptr::from_ref(v).cast::<u8>(), size_of::<T>()) }
}

/// The bytes of `v`, writable.
fn io_bytes_mut<T: PfrIo>(v: &mut T) -> &mut [u8] {
    // SAFETY: as for `io_bytes`; every bit pattern is a valid `T` (`PfrIo`), so any bytes
    // written leave a valid value.
    unsafe { core::slice::from_raw_parts_mut(ptr::from_mut(v).cast::<u8>(), size_of::<T>()) }
}

/// The user address of element `i` of the array at `base`.
fn io_uaddr<T>(base: usize, i: usize) -> Result<usize, Errno> {
    i.checked_mul(size_of::<T>())
        .and_then(|off| base.checked_add(off))
        .ok_or(Errno::EFAULT)
}

/// `COPYIN(from + i, to, sizeof(*to), flags)`.
fn pfr_copyin<T: PfrIo>(
    from: &PfrBuf<'_, T>,
    i: usize,
    to: &mut T,
    flags: i32,
) -> Result<(), Errno> {
    match (flags & PFR_FLAG_USERIOCTL != 0, from) {
        (true, PfrBuf::User(base)) => copyin(io_uaddr::<T>(*base, i)?, io_bytes_mut(to)),
        (false, PfrBuf::Kernel(a)) => {
            let src = a.get(i).ok_or(Errno::EFAULT)?;
            io_bytes_mut(to).copy_from_slice(io_bytes(src));
            Ok(())
        }
        _ => Err(Errno::EFAULT),
    }
}

/// `COPYOUT(from, to + i, sizeof(*from), flags)`.
fn pfr_copyout<T: PfrIo>(
    from: &T,
    to: &mut PfrBuf<'_, T>,
    i: usize,
    flags: i32,
) -> Result<(), Errno> {
    match (flags & PFR_FLAG_USERIOCTL != 0, to) {
        (true, PfrBuf::User(base)) => copyout(io_bytes(from), io_uaddr::<T>(*base, i)?),
        (false, PfrBuf::Kernel(a)) => {
            let dst = a.get_mut(i).ok_or(Errno::EFAULT)?;
            io_bytes_mut(dst).copy_from_slice(io_bytes(from));
            Ok(())
        }
        _ => Err(Errno::EFAULT),
    }
}

/// `YIELD(ok)`.
fn pfr_yield(ok: bool) {
    if ok {
        sched_pause(preempt);
    }
}

/// `flags & PFR_FLAG_USERIOCTL ? PR_WAITOK : PR_NOWAIT`.
fn pfr_wait(flags: i32) -> i32 {
    if flags & PFR_FLAG_USERIOCTL != 0 {
        PR_WAITOK
    } else {
        PR_NOWAIT
    }
}

/// `FILLIN_SIN(sin, addr)` on a zeroed union.
fn fillin_sin(su: &mut PfsockaddrUnion, addr: InAddr) {
    let mut sin = su.sin();
    sin.sin_len = size_of::<SockaddrIn>() as u8;
    sin.sin_family = AF_INET;
    sin.sin_addr = addr;
    su.set_sin(&sin);
}

/// `FILLIN_SIN6(sin6, addr)` on a zeroed union.
#[cfg(feature = "inet6")]
fn fillin_sin6(su: &mut PfsockaddrUnion, addr: In6Addr) {
    let mut sin6 = su.sin6();
    sin6.sin6_len = size_of::<SockaddrIn6>() as u8;
    sin6.sin6_family = AF_INET6;
    sin6.sin6_addr = addr;
    su.set_sin6(&sin6);
}

/// `SUNION2PF(su, af)`: the `pf_addr` at the union's address (`sin_addr` or `sin6_addr`).
fn sunion2pf(su: &PfsockaddrUnion, af: SaFamily) -> PfAddr {
    let off = if af == AF_INET {
        offset_of!(SockaddrIn, sin_addr)
    } else {
        offset_of!(SockaddrIn6, sin6_addr)
    };
    let mut a = PfAddr::zeroed();
    a.addr8.copy_from_slice(&su.bytes[off..off + 16]);
    a
}

/// `AF_BITS(af)`.
fn af_bits(af: u8) -> u8 {
    if af == AF_INET { 32 } else { 128 }
}

/// `ADDR_NETWORK(ad)`.
fn addr_network(ad: &PfrAddr) -> bool {
    ad.pfra_net < af_bits(ad.pfra_af)
}

/// `KENTRY_NETWORK(ke)`.
fn kentry_network(ke: &PfrKentry) -> bool {
    ke.pfrke_net.get() < af_bits(ke.pfrke_af.get())
}

/// `SLIST_FIRST` of a work queue of this file: a pool item (entry or table), which stays
/// allocated until `pfr_destroy_kentry`/`pfr_destroy_ktable`, after it is unlinked.
fn sl_first<A: SlistAdapter>(q: &SlistHead<A>) -> Option<&'static A::Elem>
where
    A::Elem: 'static,
{
    // SAFETY: see the function's doc: the element outlives every queue it is on.
    q.first().map(|e| unsafe { &*ptr::from_ref(e) })
}

/// `SLIST_FOREACH_SAFE` over a work queue of this file (the successor is read first).
fn sl_iter<A: SlistAdapter>(q: &SlistHead<A>) -> impl Iterator<Item = &'static A::Elem>
where
    A::Elem: 'static,
{
    let mut p = sl_first(q);
    core::iter::from_fn(move || {
        let e = p?;
        p = SlistHead::<A>::next(e);
        Some(e)
    })
}

/// The entry whose node pair starts with `rn`: every leaf of a table's radix tree but the
/// root leaves (which the radix functions never return) is an entry's `pfrke_node[0]`.
fn rn2ke(rn: &'static RadixNode) -> &'static PfrKentry {
    // SAFETY: `PfrKentry` is `#[repr(C)]` with `pfrke_node` first, and the only node pairs
    // `pfr_route_kentry` adds to a table's trees are entries' `pfrke_node`; an entry stays
    // allocated while it is in a tree.
    unsafe { &*ptr::from_ref(rn).cast::<PfrKentry>() }
}

/// `pfrke_rkif`/`pfrke_rifname`: the route part of a route or cost entry.
fn pfr_kentry_route(ke: &'static PfrKentry) -> &'static PfrKentryRoute {
    let t = ke.pfrke_type.get();
    if t != PFRKE_ROUTE && t != PFRKE_COST {
        panic(format_args!("pfr_kentry_route: type {t}"));
    }
    // SAFETY: an entry of type ROUTE or COST is an item of `pfr_kentry_pl[ROUTE]` or
    // `pfr_kentry_pl[COST]`, a `PfrKentryRoute` or a `PfrKentryCost`, both `#[repr(C)]` and
    // starting with the entry, then `kif` and `ifname` ("above overlaps with pfr_kentry
    // route").
    unsafe { &*ptr::from_ref(ke).cast::<PfrKentryRoute>() }
}

/// `(struct pfr_kentry_cost *)ke`: the cost entry `ke` is.
fn pfr_kentry_cost(ke: &'static PfrKentry) -> &'static PfrKentryCost {
    let t = ke.pfrke_type.get();
    if t != PFRKE_COST {
        panic(format_args!("pfr_kentry_cost: type {t}"));
    }
    // SAFETY: an entry of type COST is an item of `pfr_kentry_pl[COST]`, a `#[repr(C)]`
    // `PfrKentryCost` starting with the entry.
    unsafe { &*ptr::from_ref(ke).cast::<PfrKentryCost>() }
}

/// The key the radix trees hold for `ke`: its `pfrke_sa`, which stays in place while the
/// entry is in a tree.
fn ke_key(ke: &PfrKentry) -> *const u8 {
    ke.pfrke_sa.as_ptr().cast::<u8>().cast_const()
}

/// The radix head `h` of a table, which `pfr_create_ktable` made.
fn pfr_rnh(h: &Cell<Option<&'static RadixNodeHead>>) -> &'static RadixNodeHead {
    match h.get() {
        Some(h) => h,
        None => panic(format_args!("pf_table: table without radix head")),
    }
}

/// `pfr_gcd`.
pub fn pfr_gcd(mut m: i32, mut n: i32) -> i32 {
    while m > 0 {
        let t = n % m;
        n = m;
        m = t;
    }
    n
}

/// `pfr_initialize`: the radix code's key length, the pools and `pfr_ffaddr`.
pub fn pfr_initialize() {
    // sizeof(struct sockaddr_in6), the size of a pfsockaddr_union.
    rn_init(size_of::<PfsockaddrUnion>() as u32);

    pf_pool_init::<PfrKtable>(&PFR_KTABLE_PL, IPL_SOFTNET, 0, "pfrktable");
    pf_pool_init::<PfrKentry>(
        &PFR_KENTRY_PL[PFRKE_PLAIN as usize],
        IPL_SOFTNET,
        0,
        "pfrke_plain",
    );
    pf_pool_init::<PfrKentryRoute>(
        &PFR_KENTRY_PL[PFRKE_ROUTE as usize],
        IPL_SOFTNET,
        0,
        "pfrke_route",
    );
    pf_pool_init::<PfrKentryCost>(
        &PFR_KENTRY_PL[PFRKE_COST as usize],
        IPL_SOFTNET,
        0,
        "pfrke_cost",
    );
    pf_pool_init::<PfrKcounters>(&PFR_KCOUNTERS_PL, IPL_SOFTNET, 0, "pfrkcounters");

    PFR_FFADDR.set(PfAddr { addr8: [0xff; 16] });
}

/// `pfr_clr_addrs`: removes every address of the table `tbl`; the count in `ndel`.
pub fn pfr_clr_addrs(tbl: &mut PfrTable, ndel: Option<&mut i32>, flags: i32) -> Result<(), Errno> {
    let workq = SlistHead::<PfrKentryworkq>::new();

    accept_flags(flags, PFR_FLAG_DUMMY)?;
    pfr_validate_table(tbl, 0, flags & PFR_FLAG_USERIOCTL != 0)?;
    let Some(kt) =
        pfr_lookup_table(tbl).filter(|kt| kt.pfrkt_flags().get() & PFR_TFLAG_ACTIVE != 0)
    else {
        return Err(Errno::ESRCH);
    };
    if kt.pfrkt_flags().get() & PFR_TFLAG_CONST != 0 {
        return Err(Errno::EPERM);
    }
    pfr_enqueue_addrs(kt, &workq, ndel, false);

    if flags & PFR_FLAG_DUMMY == 0 {
        pfr_remove_kentries(kt, &workq);
        if kt.pfrkt_cnt().get() != 0 {
            crate::dpfprintf!(
                LOG_NOTICE,
                "pfr_clr_addrs: corruption detected ({}).",
                kt.pfrkt_cnt().get()
            );
            kt.pfrkt_cnt().set(0);
        }
    }
    Ok(())
}

/// `pfr_fill_feedback`: the entry `ke` as `pfr_add_addrs` reports it back in `ad`.
pub fn pfr_fill_feedback(ke: &'static PfrKentry, ad: &mut PfrAddr) {
    ad.pfra_type = ke.pfrke_type.get();

    let t = ke.pfrke_type.get();
    if t == PFRKE_COST || t == PFRKE_ROUTE {
        if t == PFRKE_COST {
            pfr_kentry_cost(ke).weight.set(ad.pfra_weight);
        }
        let ifname = pfr_kentry_route(ke).ifname.get();
        if ifname[0] != 0 {
            strlcpy(&mut ad.pfra_ifname, &ifname);
        }
    }

    match ke.pfrke_af.get() {
        AF_INET => ad.pfra_u.set_v4(ke.pfrke_sa.get().sin().sin_addr),
        #[cfg(feature = "inet6")]
        AF_INET6 => ad.pfra_u.set_v6(ke.pfrke_sa.get().sin6().sin6_addr),
        af => unhandled_af(i32::from(af)),
    }
    ad.pfra_weight = if ke.pfrke_type.get() == PFRKE_COST {
        pfr_kentry_cost(ke).weight.get()
    } else {
        0
    };
    ad.pfra_af = ke.pfrke_af.get();
    ad.pfra_net = ke.pfrke_net.get();
    if ke.pfrke_flags.get() & PFRKE_FLAG_NOT != 0 {
        ad.pfra_not = 1;
    }
    ad.pfra_fback = ke.pfrke_fb.get();
}

/// `pfr_add_addrs`: adds `size` addresses from `addr` to the table `tbl`; the count of added
/// ones in `nadd`, per-address feedback written back into `addr` with `PFR_FLAG_FEEDBACK`.
pub fn pfr_add_addrs(
    tbl: &mut PfrTable,
    addr: &mut PfrBuf<'_, PfrAddr>,
    size: i32,
    nadd: Option<&mut i32>,
    flags: i32,
) -> Result<(), Errno> {
    let workq = SlistHead::<PfrKentryworkq>::new();
    let ioq = SlistHead::<PfrKentryioq>::new();
    let mut ad = PfrAddr::default();
    let mut xadd = 0;
    let tzero = gettime();
    let user = flags & PFR_FLAG_USERIOCTL != 0;

    accept_flags(flags, PFR_FLAG_DUMMY | PFR_FLAG_FEEDBACK)?;
    pfr_validate_table(tbl, 0, user)?;
    let tmpkt =
        pfr_create_ktable(&PFR_NULLTABLE, 0, false, pfr_wait(flags)).ok_or(Errno::ENOMEM)?;

    let rv: Result<(), Errno> = 'bad: {
        for i in 0..size.max(0) as usize {
            pfr_yield(user);
            if pfr_copyin(addr, i, &mut ad, flags).is_err() {
                break 'bad Err(Errno::EFAULT);
            }
            if !pfr_validate_addr(&ad) {
                break 'bad Err(Errno::EINVAL);
            }

            let Some(ke) = pfr_create_kentry_unlocked(&mut ad, flags) else {
                break 'bad Err(Errno::ENOMEM);
            };
            ke.pfrke_fb.set(PFR_FB_NONE);
            // SAFETY: a fresh entry, on no list; it stays allocated until destroyed after
            // leaving the ioq.
            unsafe { ioq.insert_head(ke) };
        }

        net_lock();
        pf_lock();
        let kt = pfr_lookup_table(tbl);
        let Some(kt) = kt.filter(|kt| kt.pfrkt_flags().get() & PFR_TFLAG_ACTIVE != 0) else {
            pf_unlock();
            net_unlock();
            break 'bad Err(Errno::ESRCH);
        };
        if kt.pfrkt_flags().get() & PFR_TFLAG_CONST != 0 {
            pf_unlock();
            net_unlock();
            break 'bad Err(Errno::EPERM);
        }
        for ke in sl_iter(&ioq) {
            pfr_kentry_kif_ref(ke);
            let p = pfr_lookup_kentry(kt, ke, true);
            let q = pfr_lookup_kentry(tmpkt, ke, true);
            if flags & PFR_FLAG_FEEDBACK != 0 {
                if q.is_some() {
                    ke.pfrke_fb.set(PFR_FB_DUPLICATE);
                } else if let Some(p) = p {
                    if (p.pfrke_flags.get() & PFRKE_FLAG_NOT)
                        != (ke.pfrke_flags.get() & PFRKE_FLAG_NOT)
                    {
                        ke.pfrke_fb.set(PFR_FB_CONFLICT);
                    } else {
                        ke.pfrke_fb.set(PFR_FB_NONE);
                    }
                } else {
                    ke.pfrke_fb.set(PFR_FB_ADDED);
                }
            }
            if p.is_none() && q.is_none() {
                if !pfr_route_kentry(tmpkt, ke) {
                    // defer destroy after feedback is processed
                    ke.pfrke_fb.set(PFR_FB_NONE);
                } else {
                    // mark entry as added to table, so we won't kill it with rest of the ioq
                    ke.pfrke_fb.set(PFR_FB_ADDED);
                    // SAFETY: a work queue of this call; the entry is on no other.
                    unsafe { workq.insert_head(ke) };
                    xadd += 1;
                }
            }
        }
        // remove entries, which we will insert from tmpkt
        pfr_clean_node_mask(tmpkt, &workq);
        if flags & PFR_FLAG_DUMMY == 0 {
            pfr_insert_kentries(kt, &workq, tzero);
        }

        pf_unlock();
        net_unlock();

        if flags & PFR_FLAG_FEEDBACK != 0 {
            let mut i = 0;
            while let Some(ke) = sl_first(&ioq) {
                pfr_yield(user);
                pfr_fill_feedback(ke, &mut ad);
                if pfr_copyout(&ad, addr, i, flags).is_err() {
                    break 'bad Err(Errno::EFAULT);
                }
                i += 1;
                // SAFETY: the ioq holds `ke` at its head.
                unsafe { ioq.remove_head() };
                match ke.pfrke_fb.get() {
                    PFR_FB_CONFLICT | PFR_FB_DUPLICATE | PFR_FB_NONE => pfr_destroy_kentry(ke),
                    PFR_FB_ADDED if flags & PFR_FLAG_DUMMY != 0 => pfr_destroy_kentry(ke),
                    _ => {}
                }
            }
        } else {
            pfr_destroy_ioq(&ioq, flags);
        }

        if let Some(nadd) = nadd {
            *nadd = xadd;
        }

        pfr_destroy_ktable(tmpkt, false);
        return Ok(());
    };
    pfr_destroy_ioq(&ioq, flags);
    if flags & PFR_FLAG_FEEDBACK != 0 {
        pfr_reset_feedback(addr, size, flags);
    }
    pfr_destroy_ktable(tmpkt, false);
    rv
}

/// `pfr_del_addrs`: removes `size` addresses of `addr` from the table `tbl`; the count of
/// removed ones in `ndel`.
pub fn pfr_del_addrs(
    tbl: &mut PfrTable,
    addr: &mut PfrBuf<'_, PfrAddr>,
    size: i32,
    ndel: Option<&mut i32>,
    flags: i32,
) -> Result<(), Errno> {
    let workq = SlistHead::<PfrKentryworkq>::new();
    let mut ad = PfrAddr::default();
    let mut xdel = 0;
    let mut log = 1;
    let user = flags & PFR_FLAG_USERIOCTL != 0;

    accept_flags(flags, PFR_FLAG_DUMMY | PFR_FLAG_FEEDBACK)?;
    pfr_validate_table(tbl, 0, user)?;
    let Some(kt) =
        pfr_lookup_table(tbl).filter(|kt| kt.pfrkt_flags().get() & PFR_TFLAG_ACTIVE != 0)
    else {
        return Err(Errno::ESRCH);
    };
    if kt.pfrkt_flags().get() & PFR_TFLAG_CONST != 0 {
        return Err(Errno::EPERM);
    }
    // there are two algorithms to choose from here.
    // with:
    //   n: number of addresses to delete
    //   N: number of addresses in the table
    //
    // one is O(N) and is better for large 'n'
    // one is O(n*LOG(N)) and is better for small 'n'
    //
    // following code try to decide which one is best.
    let mut i = kt.pfrkt_cnt().get();
    while i > 0 {
        log += 1;
        i >>= 1;
    }
    if size > kt.pfrkt_cnt().get() / log {
        // full table scan
        pfr_mark_addrs(kt);
    } else {
        // iterate over addresses to delete
        for i in 0..size.max(0) as usize {
            pfr_yield(user);
            if pfr_copyin(addr, i, &mut ad, flags).is_err() {
                return Err(Errno::EFAULT);
            }
            if !pfr_validate_addr(&ad) {
                return Err(Errno::EINVAL);
            }
            if let Some(p) = pfr_lookup_addr(kt, &ad, true) {
                p.pfrke_flags.set(p.pfrke_flags.get() & !PFRKE_FLAG_MARK);
            }
        }
    }
    let rv: Result<(), Errno> = 'bad: {
        for i in 0..size.max(0) as usize {
            pfr_yield(user);
            if pfr_copyin(addr, i, &mut ad, flags).is_err() {
                break 'bad Err(Errno::EFAULT);
            }
            if !pfr_validate_addr(&ad) {
                break 'bad Err(Errno::EINVAL);
            }
            let p = pfr_lookup_addr(kt, &ad, true);
            if flags & PFR_FLAG_FEEDBACK != 0 {
                ad.pfra_fback = match p {
                    None => PFR_FB_NONE,
                    Some(p) if (p.pfrke_flags.get() & PFRKE_FLAG_NOT) != ad.pfra_not => {
                        PFR_FB_CONFLICT
                    }
                    Some(p) if p.pfrke_flags.get() & PFRKE_FLAG_MARK != 0 => PFR_FB_DUPLICATE,
                    Some(_) => PFR_FB_DELETED,
                };
            }
            if let Some(p) = p
                && (p.pfrke_flags.get() & PFRKE_FLAG_NOT) == ad.pfra_not
                && p.pfrke_flags.get() & PFRKE_FLAG_MARK == 0
            {
                p.pfrke_flags.set(p.pfrke_flags.get() | PFRKE_FLAG_MARK);
                // SAFETY: a work queue of this call; the mark keeps an entry from being
                // queued twice.
                unsafe { workq.insert_head(p) };
                xdel += 1;
            }
            if flags & PFR_FLAG_FEEDBACK != 0 && pfr_copyout(&ad, addr, i, flags).is_err() {
                break 'bad Err(Errno::EFAULT);
            }
        }
        if flags & PFR_FLAG_DUMMY == 0 {
            pfr_remove_kentries(kt, &workq);
        }
        if let Some(ndel) = ndel {
            *ndel = xdel;
        }
        return Ok(());
    };
    if flags & PFR_FLAG_FEEDBACK != 0 {
        pfr_reset_feedback(addr, size, flags);
    }
    rv
}

/// `pfr_set_addrs`: makes the table `tbl` hold exactly the `size` addresses of `addr`; the
/// counts in `nadd`, `ndel` and `nchange`. With `PFR_FLAG_FEEDBACK` and a nonzero `*size2`,
/// the deleted addresses are written after the given ones, `*size2` being the room for them
/// and set to the number needed.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn pfr_set_addrs(
    tbl: &mut PfrTable,
    addr: &mut PfrBuf<'_, PfrAddr>,
    size: i32,
    mut size2: Option<&mut i32>,
    nadd: Option<&mut i32>,
    ndel: Option<&mut i32>,
    nchange: Option<&mut i32>,
    flags: i32,
    ignore_pfrt_flags: u32,
) -> Result<(), Errno> {
    let addq = SlistHead::<PfrKentryworkq>::new();
    let delq = SlistHead::<PfrKentryworkq>::new();
    let changeq = SlistHead::<PfrKentryworkq>::new();
    let mut ad = PfrAddr::default();
    let (mut xadd, mut xdel, mut xchange) = (0, 0, 0);
    let tzero = gettime();
    let user = flags & PFR_FLAG_USERIOCTL != 0;

    accept_flags(flags, PFR_FLAG_DUMMY | PFR_FLAG_FEEDBACK)?;
    pfr_validate_table(tbl, ignore_pfrt_flags, user)?;
    let Some(kt) =
        pfr_lookup_table(tbl).filter(|kt| kt.pfrkt_flags().get() & PFR_TFLAG_ACTIVE != 0)
    else {
        return Err(Errno::ESRCH);
    };
    if kt.pfrkt_flags().get() & PFR_TFLAG_CONST != 0 {
        return Err(Errno::EPERM);
    }
    let tmpkt =
        pfr_create_ktable(&PFR_NULLTABLE, 0, false, pfr_wait(flags)).ok_or(Errno::ENOMEM)?;
    pfr_mark_addrs(kt);
    let rv: Result<(), Errno> = 'bad: {
        for i in 0..size.max(0) as usize {
            pfr_yield(user);
            if pfr_copyin(addr, i, &mut ad, flags).is_err() {
                break 'bad Err(Errno::EFAULT);
            }
            if !pfr_validate_addr(&ad) {
                break 'bad Err(Errno::EINVAL);
            }
            ad.pfra_fback = PFR_FB_NONE;
            'skip: {
                if let Some(p) = pfr_lookup_addr(kt, &ad, true) {
                    if p.pfrke_flags.get() & PFRKE_FLAG_MARK != 0 {
                        ad.pfra_fback = PFR_FB_DUPLICATE;
                        break 'skip;
                    }
                    p.pfrke_flags.set(p.pfrke_flags.get() | PFRKE_FLAG_MARK);
                    if (p.pfrke_flags.get() & PFRKE_FLAG_NOT) != ad.pfra_not {
                        // SAFETY: a work queue of this call; the mark keeps an entry from
                        // being queued twice.
                        unsafe { changeq.insert_head(p) };
                        ad.pfra_fback = PFR_FB_CHANGED;
                        xchange += 1;
                    }
                } else {
                    if pfr_lookup_addr(tmpkt, &ad, true).is_some() {
                        ad.pfra_fback = PFR_FB_DUPLICATE;
                        break 'skip;
                    }
                    let Some(p) = pfr_create_kentry(&mut ad) else {
                        break 'bad Err(Errno::ENOMEM);
                    };
                    if !pfr_route_kentry(tmpkt, p) {
                        pfr_destroy_kentry(p);
                        ad.pfra_fback = PFR_FB_NONE;
                        break 'skip;
                    }
                    // SAFETY: a fresh entry, on no list.
                    unsafe { addq.insert_head(p) };
                    ad.pfra_fback = PFR_FB_ADDED;
                    xadd += 1;
                    if p.pfrke_type.get() == PFRKE_COST {
                        kt.pfrkt_refcntcost.set(kt.pfrkt_refcntcost.get() + 1);
                    }
                    pfr_ktable_winfo_update(kt, p);
                }
            }
            // _skip:
            if flags & PFR_FLAG_FEEDBACK != 0 && pfr_copyout(&ad, addr, i, flags).is_err() {
                break 'bad Err(Errno::EFAULT);
            }
        }
        pfr_enqueue_addrs(kt, &delq, Some(&mut xdel), ENQUEUE_UNMARKED_ONLY);
        if flags & PFR_FLAG_FEEDBACK != 0
            && let Some(s2) = size2.as_deref_mut()
            && *s2 != 0
        {
            if *s2 < size + xdel {
                *s2 = size + xdel;
                break 'bad Ok(());
            }
            for (i, p) in sl_iter(&delq).enumerate() {
                pfr_copyout_addr(&mut ad, Some(p));
                ad.pfra_fback = PFR_FB_DELETED;
                if pfr_copyout(&ad, addr, size as usize + i, flags).is_err() {
                    break 'bad Err(Errno::EFAULT);
                }
            }
        }
        pfr_clean_node_mask(tmpkt, &addq);
        if flags & PFR_FLAG_DUMMY == 0 {
            pfr_insert_kentries(kt, &addq, tzero);
            pfr_remove_kentries(kt, &delq);
            pfr_clstats_kentries(&changeq, tzero, INVERT_NEG_FLAG);
        } else {
            pfr_destroy_kentries(&addq);
        }
        if let Some(nadd) = nadd {
            *nadd = xadd;
        }
        if let Some(ndel) = ndel {
            *ndel = xdel;
        }
        if let Some(nchange) = nchange {
            *nchange = xchange;
        }
        if flags & PFR_FLAG_FEEDBACK != 0
            && let Some(s2) = size2
        {
            *s2 = size + xdel;
        }
        pfr_destroy_ktable(tmpkt, false);
        return Ok(());
    };
    pfr_clean_node_mask(tmpkt, &addq);
    pfr_destroy_kentries(&addq);
    if flags & PFR_FLAG_FEEDBACK != 0 {
        pfr_reset_feedback(addr, size, flags);
    }
    pfr_destroy_ktable(tmpkt, false);
    rv
}

/// `pfr_tst_addrs`: tests which of the `size` host addresses of `addr` the table `tbl`
/// matches (feedback in each), the count of matches in `nmatch`; with `PFR_FLAG_REPLACE` each
/// address is replaced by the entry that matched it.
pub fn pfr_tst_addrs(
    tbl: &mut PfrTable,
    addr: &mut PfrBuf<'_, PfrAddr>,
    size: i32,
    nmatch: Option<&mut i32>,
    flags: i32,
) -> Result<(), Errno> {
    let mut ad = PfrAddr::default();
    let mut xmatch = 0;

    accept_flags(flags, PFR_FLAG_REPLACE)?;
    pfr_validate_table(tbl, 0, false)?;
    let Some(kt) =
        pfr_lookup_table(tbl).filter(|kt| kt.pfrkt_flags().get() & PFR_TFLAG_ACTIVE != 0)
    else {
        return Err(Errno::ESRCH);
    };

    for i in 0..size.max(0) as usize {
        pfr_yield(flags & PFR_FLAG_USERIOCTL != 0);
        if pfr_copyin(addr, i, &mut ad, flags).is_err() {
            return Err(Errno::EFAULT);
        }
        if !pfr_validate_addr(&ad) {
            return Err(Errno::EINVAL);
        }
        if addr_network(&ad) {
            return Err(Errno::EINVAL);
        }
        let p = pfr_lookup_addr(kt, &ad, false);
        if flags & PFR_FLAG_REPLACE != 0 {
            pfr_copyout_addr(&mut ad, p);
        }
        ad.pfra_fback = match p {
            None => PFR_FB_NONE,
            Some(p) if p.pfrke_flags.get() & PFRKE_FLAG_NOT != 0 => PFR_FB_NOTMATCH,
            Some(_) => PFR_FB_MATCH,
        };
        if p.is_some_and(|p| p.pfrke_flags.get() & PFRKE_FLAG_NOT == 0) {
            xmatch += 1;
        }
        if pfr_copyout(&ad, addr, i, flags).is_err() {
            return Err(Errno::EFAULT);
        }
    }
    if let Some(nmatch) = nmatch {
        *nmatch = xmatch;
    }
    Ok(())
}

/// Walks the radix tree `h` with `pfr_walktree` and `w`.
fn pfr_walk(h: &'static RadixNodeHead, w: &mut PfrWalktree<'_, '_>) -> Result<(), Errno> {
    rn_walktree(h, |rn, id| pfr_walktree(rn, w, id))
}

/// `pfr_get_addrs`: copies the addresses of the table `tbl` out to `addr` (`copyout`, a user
/// buffer), when `*size` leaves room for them; `*size` is set to their number.
pub fn pfr_get_addrs(
    tbl: &mut PfrTable,
    addr: &mut PfrBuf<'_, PfrAddr>,
    size: &mut i32,
    flags: i32,
) -> Result<(), Errno> {
    accept_flags(flags, 0)?;
    pfr_validate_table(tbl, 0, false)?;
    let Some(kt) =
        pfr_lookup_table(tbl).filter(|kt| kt.pfrkt_flags().get() & PFR_TFLAG_ACTIVE != 0)
    else {
        return Err(Errno::ESRCH);
    };
    if kt.pfrkt_cnt().get() > *size {
        *size = kt.pfrkt_cnt().get();
        return Ok(());
    }

    let mut w = PfrWalktree::new(PfrwOp::GetAddrs);
    w.pfrw_1 = Pfrw1::Addr(addr, 0);
    w.pfrw_free = kt.pfrkt_cnt().get();
    w.pfrw_flags = flags;
    let mut rv = pfr_walk(pfr_rnh(&kt.pfrkt_ip4), &mut w);
    if rv.is_ok() {
        rv = pfr_walk(pfr_rnh(&kt.pfrkt_ip6), &mut w);
    }
    rv?;

    if w.pfrw_free != 0 {
        crate::dpfprintf!(
            LOG_ERR,
            "pfr_get_addrs: corruption detected ({})",
            w.pfrw_free
        );
        return Err(Errno::ENOTTY);
    }
    *size = kt.pfrkt_cnt().get();
    Ok(())
}

/// `pfr_get_astats`: copies the addresses of the table `tbl` with their counters out to
/// `addr`, when `*size` leaves room for them; `*size` is set to their number.
/// `PFR_FLAG_CLSTATS` clears the counters afterwards.
pub fn pfr_get_astats(
    tbl: &mut PfrTable,
    addr: &mut PfrBuf<'_, PfrAstats>,
    size: &mut i32,
    flags: i32,
) -> Result<(), Errno> {
    let workq = SlistHead::<PfrKentryworkq>::new();
    let tzero = gettime();

    pfr_validate_table(tbl, 0, false)?;
    let Some(kt) =
        pfr_lookup_table(tbl).filter(|kt| kt.pfrkt_flags().get() & PFR_TFLAG_ACTIVE != 0)
    else {
        return Err(Errno::ESRCH);
    };
    if kt.pfrkt_cnt().get() > *size {
        *size = kt.pfrkt_cnt().get();
        return Ok(());
    }

    let mut w = PfrWalktree::new(PfrwOp::GetAstats);
    w.pfrw_1 = Pfrw1::Astats(addr, 0);
    w.pfrw_free = kt.pfrkt_cnt().get();
    w.pfrw_flags = flags;
    let mut rv = pfr_walk(pfr_rnh(&kt.pfrkt_ip4), &mut w);
    if rv.is_ok() {
        rv = pfr_walk(pfr_rnh(&kt.pfrkt_ip6), &mut w);
    }
    if rv.is_ok() && flags & PFR_FLAG_CLSTATS != 0 {
        pfr_enqueue_addrs(kt, &workq, None, false);
        pfr_clstats_kentries(&workq, tzero, false);
    }
    rv?;

    if w.pfrw_free != 0 {
        crate::dpfprintf!(
            LOG_ERR,
            "pfr_get_astats: corruption detected ({})",
            w.pfrw_free
        );
        return Err(Errno::ENOTTY);
    }
    *size = kt.pfrkt_cnt().get();
    Ok(())
}

/// `pfr_clr_astats`: clears the counters of the `size` addresses of `addr` in the table
/// `tbl`; the count in `nzero`.
pub fn pfr_clr_astats(
    tbl: &mut PfrTable,
    addr: &mut PfrBuf<'_, PfrAddr>,
    size: i32,
    nzero: Option<&mut i32>,
    flags: i32,
) -> Result<(), Errno> {
    let workq = SlistHead::<PfrKentryworkq>::new();
    let mut ad = PfrAddr::default();
    let mut xzero = 0;

    accept_flags(flags, PFR_FLAG_DUMMY | PFR_FLAG_FEEDBACK)?;
    pfr_validate_table(tbl, 0, false)?;
    let Some(kt) =
        pfr_lookup_table(tbl).filter(|kt| kt.pfrkt_flags().get() & PFR_TFLAG_ACTIVE != 0)
    else {
        return Err(Errno::ESRCH);
    };
    let rv: Result<(), Errno> = 'bad: {
        for i in 0..size.max(0) as usize {
            pfr_yield(flags & PFR_FLAG_USERIOCTL != 0);
            if pfr_copyin(addr, i, &mut ad, flags).is_err() {
                break 'bad Err(Errno::EFAULT);
            }
            if !pfr_validate_addr(&ad) {
                break 'bad Err(Errno::EINVAL);
            }
            let p = pfr_lookup_addr(kt, &ad, true);
            if flags & PFR_FLAG_FEEDBACK != 0 {
                ad.pfra_fback = if p.is_some() {
                    PFR_FB_CLEARED
                } else {
                    PFR_FB_NONE
                };
                if pfr_copyout(&ad, addr, i, flags).is_err() {
                    break 'bad Err(Errno::EFAULT);
                }
            }
            if let Some(p) = p {
                // SAFETY: a work queue of this call (an address given twice is queued twice,
                // as in C).
                unsafe { workq.insert_head(p) };
                xzero += 1;
            }
        }

        if flags & PFR_FLAG_DUMMY == 0 {
            pfr_clstats_kentries(&workq, gettime(), false);
        }
        if let Some(nzero) = nzero {
            *nzero = xzero;
        }
        return Ok(());
    };
    if flags & PFR_FLAG_FEEDBACK != 0 {
        pfr_reset_feedback(addr, size, flags);
    }
    rv
}

/// `pfr_validate_addr`: `true` (the C's 0) when `ad` is a valid table address; `false` (the
/// C's -1) for an unknown family, a prefix too long, host bits set, or bad flags.
pub fn pfr_validate_addr(ad: &PfrAddr) -> bool {
    match ad.pfra_af {
        AF_INET => {
            if ad.pfra_net > 32 {
                return false;
            }
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            if ad.pfra_net > 128 {
                return false;
            }
        }
        _ => return false,
    }
    let net = usize::from(ad.pfra_net);
    let bytes = &ad.pfra_u.addr8;
    if net < 128 && bytes[net / 8] & (0xff >> (net % 8)) != 0 {
        return false;
    }
    if bytes[net.div_ceil(8)..].iter().any(|&b| b != 0) {
        return false;
    }
    if ad.pfra_not != 0 && ad.pfra_not != 1 {
        return false;
    }
    if ad.pfra_fback != PFR_FB_NONE {
        return false;
    }
    if ad.pfra_type >= PFRKE_MAX {
        return false;
    }
    true
}

/// `pfr_enqueue_addrs`: queues the entries of `kt` (only the unmarked ones with `sweep`) on
/// `workq`; their number in `naddr`.
pub fn pfr_enqueue_addrs(
    kt: &'static PfrKtable,
    workq: &SlistHead<PfrKentryworkq>,
    naddr: Option<&mut i32>,
    sweep: bool,
) {
    workq.init();
    let mut w = PfrWalktree::new(if sweep {
        PfrwOp::Sweep
    } else {
        PfrwOp::Enqueue
    });
    w.pfrw_1 = Pfrw1::Workq(workq);
    if let Some(h) = kt.pfrkt_ip4.get()
        && pfr_walk(h, &mut w).is_err()
    {
        crate::dpfprintf!(LOG_ERR, "pfr_enqueue_addrs: IPv4 walktree failed.");
    }
    if let Some(h) = kt.pfrkt_ip6.get()
        && pfr_walk(h, &mut w).is_err()
    {
        crate::dpfprintf!(LOG_ERR, "pfr_enqueue_addrs: IPv6 walktree failed.");
    }
    if let Some(naddr) = naddr {
        *naddr = w.pfrw_free;
    }
}

/// `pfr_mark_addrs`: clears the mark of every entry of `kt`.
pub fn pfr_mark_addrs(kt: &'static PfrKtable) {
    let mut w = PfrWalktree::new(PfrwOp::Mark);
    if pfr_walk(pfr_rnh(&kt.pfrkt_ip4), &mut w).is_err() {
        crate::dpfprintf!(LOG_ERR, "pfr_mark_addrs: IPv4 walktree failed.");
    }
    if pfr_walk(pfr_rnh(&kt.pfrkt_ip6), &mut w).is_err() {
        crate::dpfprintf!(LOG_ERR, "pfr_mark_addrs: IPv6 walktree failed.");
    }
}

/// `pfr_lookup_addr`: the entry of `kt` for `ad`: for a network, the entry of exactly that
/// network; for a host, the best match (`None` with `exact` when that is a network).
pub fn pfr_lookup_addr(
    kt: &'static PfrKtable,
    ad: &PfrAddr,
    exact: bool,
) -> Option<&'static PfrKentry> {
    let mut sa = PfsockaddrUnion::default();
    let mut mask = PfsockaddrUnion::default();

    let head = match ad.pfra_af {
        AF_INET => {
            fillin_sin(&mut sa, ad.pfra_ip4addr());
            pfr_rnh(&kt.pfrkt_ip4)
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            fillin_sin6(&mut sa, ad.pfra_ip6addr());
            pfr_rnh(&kt.pfrkt_ip6)
        }
        af => unhandled_af(i32::from(af)),
    };
    if addr_network(ad) {
        pfr_prepare_network(&mut mask, ad.pfra_af, i32::from(ad.pfra_net));
        // SAFETY: the table's tree holds entries (the radix module's invariant); the key and
        // mask are 28-byte unions, readable for the radix key length.
        unsafe { rn_lookup(sa.bytes.as_ptr(), mask.bytes.as_ptr(), head) }.map(rn2ke)
    } else {
        // SAFETY: as above.
        let ke = unsafe { rn_match(sa.bytes.as_ptr(), head) }.map(rn2ke);
        ke.filter(|ke| !(exact && kentry_network(ke)))
    }
}

/// `pfr_lookup_kentry`: as `pfr_lookup_addr`, for the address of the entry `key`.
pub fn pfr_lookup_kentry(
    kt: &'static PfrKtable,
    key: &PfrKentry,
    exact: bool,
) -> Option<&'static PfrKentry> {
    let mut mask = PfsockaddrUnion::default();

    let head = match key.pfrke_af.get() {
        AF_INET => pfr_rnh(&kt.pfrkt_ip4),
        #[cfg(feature = "inet6")]
        AF_INET6 => pfr_rnh(&kt.pfrkt_ip6),
        af => unhandled_af(i32::from(af)),
    };
    if kentry_network(key) {
        pfr_prepare_network(
            &mut mask,
            key.pfrke_af.get(),
            i32::from(key.pfrke_net.get()),
        );
        // SAFETY: as in `pfr_lookup_addr`; the key is the entry's 28-byte union.
        unsafe { rn_lookup(ke_key(key), mask.bytes.as_ptr(), head) }.map(rn2ke)
    } else {
        // SAFETY: as above.
        let ke = unsafe { rn_match(ke_key(key), head) }.map(rn2ke);
        ke.filter(|ke| !(exact && kentry_network(ke)))
    }
}

/// An item of `pfr_kentry_pl[type]` (zeroed), as its common part.
fn pfr_kentry_get(type_: u8, flags: i32) -> Option<&'static PfrKentry> {
    let pp = &PFR_KENTRY_PL[usize::from(type_)];
    match type_ {
        PFRKE_PLAIN => pf_pool_get::<PfrKentry>(pp, flags),
        PFRKE_ROUTE => pf_pool_get::<PfrKentryRoute>(pp, flags).map(|r| {
            // SAFETY: `PfrKentryRoute` is `#[repr(C)]` and starts with the entry.
            unsafe { &*ptr::from_ref(r).cast::<PfrKentry>() }
        }),
        _ => pf_pool_get::<PfrKentryCost>(pp, flags).map(|c| {
            // SAFETY: `PfrKentryCost` is `#[repr(C)]` and starts with the entry.
            unsafe { &*ptr::from_ref(c).cast::<PfrKentry>() }
        }),
    }
}

/// `pfr_create_kentry`: a new entry for `ad` (its weight made 1 when 0), with a reference on
/// its interface for route and cost entries.
pub fn pfr_create_kentry(ad: &mut PfrAddr) -> Option<&'static PfrKentry> {
    if ad.pfra_type >= PFRKE_MAX {
        panic(format_args!("unknown pfra_type {}", ad.pfra_type));
    }

    let ke = pfr_kentry_get(ad.pfra_type, PR_NOWAIT | PR_ZERO)?;

    ke.pfrke_type.set(ad.pfra_type);

    // set weight allowing implicit weights
    if ad.pfra_weight == 0 {
        ad.pfra_weight = 1;
    }

    match ke.pfrke_type.get() {
        PFRKE_PLAIN => {}
        t => {
            if t == PFRKE_COST {
                pfr_kentry_cost(ke).weight.set(ad.pfra_weight);
            }
            let kr = pfr_kentry_route(ke);
            if ad.pfra_ifname[0] != 0 {
                kr.kif.set(pfi_kif_get(&ad.pfra_ifname, None));
            }
            if let Some(kif) = kr.kif.get() {
                pfi_kif_ref(kif, PFI_KIF_REF_ROUTE);
            }
        }
    }

    let mut sa = ke.pfrke_sa.get();
    match ad.pfra_af {
        AF_INET => fillin_sin(&mut sa, ad.pfra_ip4addr()),
        #[cfg(feature = "inet6")]
        AF_INET6 => fillin_sin6(&mut sa, ad.pfra_ip6addr()),
        af => unhandled_af(i32::from(af)),
    }
    ke.pfrke_sa.set(sa);
    ke.pfrke_af.set(ad.pfra_af);
    ke.pfrke_net.set(ad.pfra_net);
    if ad.pfra_not != 0 {
        ke.pfrke_flags.set(ke.pfrke_flags.get() | PFRKE_FLAG_NOT);
    }
    Some(ke)
}

/// `pfr_create_kentry_unlocked`: as `pfr_create_kentry` without the net lock: the interface
/// name is only recorded, `pfr_kentry_kif_ref` looks it up later. Sleeps for memory with
/// `PFR_FLAG_USERIOCTL`.
pub fn pfr_create_kentry_unlocked(ad: &mut PfrAddr, flags: i32) -> Option<&'static PfrKentry> {
    let mut mflags = PR_ZERO;

    if ad.pfra_type >= PFRKE_MAX {
        panic(format_args!("unknown pfra_type {}", ad.pfra_type));
    }

    if flags & PFR_FLAG_USERIOCTL != 0 {
        mflags |= PR_WAITOK;
    } else {
        mflags |= PR_NOWAIT;
    }

    let ke = pfr_kentry_get(ad.pfra_type, mflags)?;

    ke.pfrke_type.set(ad.pfra_type);

    // set weight allowing implicit weights
    if ad.pfra_weight == 0 {
        ad.pfra_weight = 1;
    }

    match ke.pfrke_type.get() {
        PFRKE_PLAIN => {}
        t => {
            if t == PFRKE_COST {
                pfr_kentry_cost(ke).weight.set(ad.pfra_weight);
            }
            if ad.pfra_ifname[0] != 0 {
                let kr = pfr_kentry_route(ke);
                let mut ifname = kr.ifname.get();
                strlcpy(&mut ifname, &ad.pfra_ifname);
                kr.ifname.set(ifname);
            }
        }
    }

    let mut sa = ke.pfrke_sa.get();
    match ad.pfra_af {
        AF_INET => fillin_sin(&mut sa, ad.pfra_ip4addr()),
        #[cfg(feature = "inet6")]
        AF_INET6 => fillin_sin6(&mut sa, ad.pfra_ip6addr()),
        af => unhandled_af(i32::from(af)),
    }
    ke.pfrke_sa.set(sa);
    ke.pfrke_af.set(ad.pfra_af);
    ke.pfrke_net.set(ad.pfra_net);
    if ad.pfra_not != 0 {
        ke.pfrke_flags.set(ke.pfrke_flags.get() | PFRKE_FLAG_NOT);
    }
    Some(ke)
}

/// `pfr_kentry_kif_ref`: looks up the interface a route or cost entry made by
/// `pfr_create_kentry_unlocked` names, and references it.
pub fn pfr_kentry_kif_ref(ke: &'static PfrKentry) {
    net_assert_locked("pfr_kentry_kif_ref");
    let t = ke.pfrke_type.get();
    if t == PFRKE_COST || t == PFRKE_ROUTE {
        let kr = pfr_kentry_route(ke);
        let ifname = kr.ifname.get();
        if ifname[0] != 0 {
            kr.kif.set(pfi_kif_get(&ifname, None));
        }
        if let Some(kif) = kr.kif.get() {
            pfi_kif_ref(kif, PFI_KIF_REF_ROUTE);
        }
    }
}

/// `pfr_destroy_kentries`: destroys the entries of `workq`, emptying it.
pub fn pfr_destroy_kentries(workq: &SlistHead<PfrKentryworkq>) {
    while let Some(p) = sl_first(workq) {
        pfr_yield(true);
        // SAFETY: the queue holds `p` at its head.
        unsafe { workq.remove_head() };
        pfr_destroy_kentry(p);
    }
}

/// `pfr_destroy_ioq`: empties `ioq`, destroying the entries that did not make it to the table
/// (or all of them with `PFR_FLAG_DUMMY`).
pub fn pfr_destroy_ioq(ioq: &SlistHead<PfrKentryioq>, flags: i32) {
    while let Some(p) = sl_first(ioq) {
        pfr_yield(flags & PFR_FLAG_USERIOCTL != 0);
        // SAFETY: the queue holds `p` at its head.
        unsafe { ioq.remove_head() };
        // we destroy only those entries, which did not make it to table
        if p.pfrke_fb.get() != PFR_FB_ADDED || flags & PFR_FLAG_DUMMY != 0 {
            pfr_destroy_kentry(p);
        }
    }
}

/// `pfr_destroy_kentry`: frees the entry `ke`, its counters and its interface reference. The
/// entry is in no tree and no queue any more.
pub fn pfr_destroy_kentry(ke: &'static PfrKentry) {
    if let Some(c) = ke.pfrke_counters.get() {
        pf_pool_put(&PFR_KCOUNTERS_PL, c);
    }
    let t = ke.pfrke_type.get();
    if t == PFRKE_COST || t == PFRKE_ROUTE {
        pfi_kif_unref(pfr_kentry_route(ke).kif.get(), PFI_KIF_REF_ROUTE);
    }
    pf_pool_put(&PFR_KENTRY_PL[usize::from(t)], ke);
}

/// `pfr_insert_kentries`: routes the entries of `workq` into `kt` (stopping at the first that
/// fails), starting their statistics at `tzero`.
pub fn pfr_insert_kentries(kt: &'static PfrKtable, workq: &SlistHead<PfrKentryworkq>, tzero: Time) {
    let mut n = 0;

    for p in sl_iter(workq) {
        if !pfr_route_kentry(kt, p) {
            crate::dpfprintf!(
                LOG_ERR,
                "pfr_insert_kentries: cannot route entry (code={}).",
                -1
            );
            break;
        }
        p.pfrke_tzero.set(tzero);
        n += 1;
        if p.pfrke_type.get() == PFRKE_COST {
            kt.pfrkt_refcntcost.set(kt.pfrkt_refcntcost.get() + 1);
        }
        pfr_ktable_winfo_update(kt, p);
        pfr_yield(true);
    }
    kt.pfrkt_cnt().set(kt.pfrkt_cnt().get() + n);
}

/// `pfr_insert_kentry`: adds the address `ad` to `kt` unless it is there (pf's overload
/// tables and source limiters). `EINVAL` without memory; the C's -1 (`ERESTART`) when the
/// entry cannot be routed.
pub fn pfr_insert_kentry(kt: &'static PfrKtable, ad: &PfrAddr, tzero: Time) -> Result<(), Errno> {
    let mut ad = *ad;

    if pfr_lookup_addr(kt, &ad, true).is_some() {
        return Ok(());
    }
    let Some(p) = pfr_create_kentry(&mut ad) else {
        return Err(Errno::EINVAL);
    };

    if !pfr_route_kentry(kt, p) {
        // The C returns pfr_route_kentry's -1 and keeps the entry.
        return Err(Errno::ERESTART);
    }

    p.pfrke_tzero.set(tzero);
    if p.pfrke_type.get() == PFRKE_COST {
        kt.pfrkt_refcntcost.set(kt.pfrkt_refcntcost.get() + 1);
    }
    kt.pfrkt_cnt().set(kt.pfrkt_cnt().get() + 1);
    pfr_ktable_winfo_update(kt, p);

    Ok(())
}

/// `pfr_remove_kentry`: removes the address `ad` from `kt`; `ESRCH` when it is not there (or
/// is a negated entry).
pub fn pfr_remove_kentry(kt: &'static PfrKtable, ad: &PfrAddr) -> Result<(), Errno> {
    let workq = SlistHead::<PfrKentryworkq>::new();

    let p = pfr_lookup_addr(kt, ad, true);
    let Some(p) = p.filter(|p| p.pfrke_flags.get() & PFRKE_FLAG_NOT == 0) else {
        return Err(Errno::ESRCH);
    };

    if p.pfrke_flags.get() & PFRKE_FLAG_MARK != 0 {
        return Ok(());
    }

    p.pfrke_flags.set(p.pfrke_flags.get() | PFRKE_FLAG_MARK);
    // SAFETY: a work queue of this call.
    unsafe { workq.insert_head(p) };
    pfr_remove_kentries(kt, &workq);

    Ok(())
}

/// `pfr_remove_kentries`: unroutes the entries of `workq` from `kt` and destroys them, then
/// recomputes the table's weights.
pub fn pfr_remove_kentries(kt: &'static PfrKtable, workq: &SlistHead<PfrKentryworkq>) {
    let addrq = SlistHead::<PfrKentryworkq>::new();
    let mut n = 0;

    for p in sl_iter(workq) {
        pfr_unroute_kentry(kt, p);
        n += 1;
        pfr_yield(true);
        if p.pfrke_type.get() == PFRKE_COST {
            kt.pfrkt_refcntcost
                .set(kt.pfrkt_refcntcost.get().wrapping_sub(1));
        }
    }
    kt.pfrkt_cnt().set(kt.pfrkt_cnt().get() - n);
    pfr_destroy_kentries(workq);

    // update maxweight and gcd for load balancing
    if kt.pfrkt_refcntcost.get() > 0 {
        kt.pfrkt_gcdweight.set(0);
        kt.pfrkt_maxweight.set(1);
        pfr_enqueue_addrs(kt, &addrq, None, false);
        for p in sl_iter(&addrq) {
            pfr_ktable_winfo_update(kt, p);
        }
    }
}

/// `pfr_clean_node_mask`: unroutes the entries of `workq` from `kt`.
pub fn pfr_clean_node_mask(kt: &'static PfrKtable, workq: &SlistHead<PfrKentryworkq>) {
    for p in sl_iter(workq) {
        pfr_unroute_kentry(kt, p);
    }
}

/// `pfr_clstats_kentries`: clears the counters of the entries of `workq`, flipping their
/// negation with `negchange`.
pub fn pfr_clstats_kentries(workq: &SlistHead<PfrKentryworkq>, tzero: Time, negchange: bool) {
    for p in sl_iter(workq) {
        if negchange {
            p.pfrke_flags.set(p.pfrke_flags.get() ^ PFRKE_FLAG_NOT);
        }
        if let Some(c) = p.pfrke_counters.get() {
            pf_pool_put(&PFR_KCOUNTERS_PL, c);
            p.pfrke_counters.set(None);
        }
        p.pfrke_tzero.set(tzero);
    }
}

/// `pfr_reset_feedback`: clears the feedback of the `size` addresses of `addr` (stopping at
/// the first copy that fails).
pub fn pfr_reset_feedback(addr: &mut PfrBuf<'_, PfrAddr>, size: i32, flags: i32) {
    let mut ad = PfrAddr::default();

    for i in 0..size.max(0) as usize {
        pfr_yield(flags & PFR_FLAG_USERIOCTL != 0);
        if pfr_copyin(addr, i, &mut ad, flags).is_err() {
            break;
        }
        ad.pfra_fback = PFR_FB_NONE;
        if pfr_copyout(&ad, addr, i, flags).is_err() {
            break;
        }
    }
}

/// `pfr_prepare_network`: the netmask of a `net`-bit prefix of family `af` in `sa`.
pub fn pfr_prepare_network(sa: &mut PfsockaddrUnion, af: SaFamily, net: i32) {
    *sa = PfsockaddrUnion::default();
    match af {
        AF_INET => {
            let mut sin = SockaddrIn {
                sin_len: size_of::<SockaddrIn>() as u8,
                sin_family: AF_INET,
                ..SockaddrIn::default()
            };
            sin.sin_addr.s_addr = if net != 0 {
                (u32::MAX << (32 - net)).to_be()
            } else {
                0
            };
            sa.set_sin(&sin);
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            let mut sin6 = SockaddrIn6 {
                sin6_len: size_of::<SockaddrIn6>() as u8,
                sin6_family: AF_INET6,
                ..SockaddrIn6::zeroed()
            };
            let mut net = net;
            for i in 0..4 {
                if net <= 32 {
                    sin6.sin6_addr.set_s6_addr32(
                        i,
                        if net != 0 {
                            (u32::MAX << (32 - net)).to_be()
                        } else {
                            0
                        },
                    );
                    break;
                }
                sin6.sin6_addr.set_s6_addr32(i, 0xffff_ffff);
                net -= 32;
            }
            sa.set_sin6(&sin6);
        }
        _ => unhandled_af(i32::from(af)),
    }
}

/// `pfr_route_kentry`: adds the entry `ke` to the radix tree of its family in `kt`. `true`
/// (the C's 0) when added, `false` (the C's -1) when the radix code refuses it (the key is
/// there, or no memory).
pub fn pfr_route_kentry(kt: &'static PfrKtable, ke: &'static PfrKentry) -> bool {
    let mut mask = PfsockaddrUnion::default();

    // bzero(ke->pfrke_node, sizeof(ke->pfrke_node))
    for n in &ke.pfrke_node {
        n.rn_mklist.set(ptr::null());
        n.rn_p.set(ptr::null());
        n.rn_b.set(0);
        n.rn_bmask.set(0);
        n.rn_flags.set(0);
        n.rn_key.set(ptr::null());
        n.rn_mask.set(ptr::null());
        n.rn_dupedkey.set(ptr::null());
        n.rn_off.set(0);
        n.rn_l.set(ptr::null());
        n.rn_r.set(ptr::null());
    }
    let head = match ke.pfrke_af.get() {
        AF_INET => pfr_rnh(&kt.pfrkt_ip4),
        #[cfg(feature = "inet6")]
        AF_INET6 => pfr_rnh(&kt.pfrkt_ip6),
        af => unhandled_af(i32::from(af)),
    };

    let rn = if kentry_network(ke) {
        pfr_prepare_network(&mut mask, ke.pfrke_af.get(), i32::from(ke.pfrke_net.get()));
        // SAFETY: the tree invariant holds for the table's head; the key is the entry's
        // union and the node pair its own, both staying in place until `pfr_unroute_kentry`;
        // the mask is copied by the radix code.
        unsafe { rn_addroute(ke_key(ke), mask.bytes.as_ptr(), head, &ke.pfrke_node, 0) }
    } else {
        // SAFETY: as above, without a mask.
        unsafe { rn_addroute(ke_key(ke), ptr::null(), head, &ke.pfrke_node, 0) }
    };

    rn.is_some()
}

/// `pfr_unroute_kentry`: removes the entry `ke` from the radix tree of its family in `kt`.
/// `true` (the C's 0) when removed, `false` (the C's -1) when it was not there.
pub fn pfr_unroute_kentry(kt: &'static PfrKtable, ke: &'static PfrKentry) -> bool {
    let mut mask = PfsockaddrUnion::default();

    let head = match ke.pfrke_af.get() {
        AF_INET => pfr_rnh(&kt.pfrkt_ip4),
        #[cfg(feature = "inet6")]
        AF_INET6 => pfr_rnh(&kt.pfrkt_ip6),
        af => unhandled_af(i32::from(af)),
    };

    let rn = if kentry_network(ke) {
        pfr_prepare_network(&mut mask, ke.pfrke_af.get(), i32::from(ke.pfrke_net.get()));
        // SAFETY: the tree invariant holds for the table's head; the key is the entry's union.
        unsafe { rn_delete(ke_key(ke), mask.bytes.as_ptr(), head, None) }
    } else {
        // SAFETY: as above.
        unsafe { rn_delete(ke_key(ke), ptr::null(), head, None) }
    };

    if rn.is_none() {
        crate::dpfprintf!(LOG_ERR, "pfr_unroute_kentry: delete failed.\n");
        return false;
    }
    true
}

/// `pfr_copyout_addr`: the entry `ke` as a `pfr_addr` in `ad` (all zero without an entry).
pub fn pfr_copyout_addr(ad: &mut PfrAddr, ke: Option<&'static PfrKentry>) {
    *ad = PfrAddr::default();
    let Some(ke) = ke else {
        return;
    };
    ad.pfra_af = ke.pfrke_af.get();
    ad.pfra_net = ke.pfrke_net.get();
    ad.pfra_type = ke.pfrke_type.get();
    if ke.pfrke_flags.get() & PFRKE_FLAG_NOT != 0 {
        ad.pfra_not = 1;
    }

    match ad.pfra_af {
        AF_INET => ad.pfra_u.set_v4(ke.pfrke_sa.get().sin().sin_addr),
        #[cfg(feature = "inet6")]
        AF_INET6 => ad.pfra_u.set_v6(ke.pfrke_sa.get().sin6().sin6_addr),
        af => unhandled_af(i32::from(af)),
    }
    if let Some(c) = ke.pfrke_counters.get() {
        ad.pfra_states = c.states.get() as u32;
    }
    let t = ke.pfrke_type.get();
    if t == PFRKE_COST || t == PFRKE_ROUTE {
        if t == PFRKE_COST {
            ad.pfra_weight = pfr_kentry_cost(ke).weight.get();
        }
        if let Some(kif) = pfr_kentry_route(ke).kif.get() {
            strlcpy(&mut ad.pfra_ifname, &kif.pfik_name);
        }
    }
}

/// `pfr_walktree`: what the table walks do with each entry, after `w.pfrw_op`.
fn pfr_walktree(
    rn: &'static RadixNode,
    w: &mut PfrWalktree<'_, '_>,
    _id: u32,
) -> Result<(), Errno> {
    let ke = rn2ke(rn);
    let mut mask = PfsockaddrUnion::default();
    let flags = w.pfrw_flags;

    match w.pfrw_op {
        PfrwOp::Mark => {
            ke.pfrke_flags.set(ke.pfrke_flags.get() & !PFRKE_FLAG_MARK);
        }
        PfrwOp::Sweep | PfrwOp::Enqueue => {
            if w.pfrw_op == PfrwOp::Sweep && ke.pfrke_flags.get() & PFRKE_FLAG_MARK != 0 {
                return Ok(());
            }
            if let Pfrw1::Workq(workq) = w.pfrw_1 {
                // SAFETY: the queue being built by `pfr_enqueue_addrs`; each entry is visited
                // once.
                unsafe { workq.insert_head(ke) };
            }
            w.pfrw_free += 1;
        }
        PfrwOp::GetAddrs => {
            let free = w.pfrw_free;
            w.pfrw_free -= 1;
            if free > 0
                && let Pfrw1::Addr(buf, i) = &mut w.pfrw_1
            {
                let mut ad = PfrAddr::default();

                pfr_copyout_addr(&mut ad, Some(ke));
                // The C copies out with copyout(9), whatever the flags.
                match buf {
                    PfrBuf::User(base) => {
                        copyout(io_bytes(&ad), io_uaddr::<PfrAddr>(*base, *i)?)
                            .map_err(|_| Errno::EFAULT)?;
                    }
                    PfrBuf::Kernel(_) => return Err(Errno::EFAULT),
                }
                *i += 1;
            }
        }
        PfrwOp::GetAstats => {
            let free = w.pfrw_free;
            w.pfrw_free -= 1;
            if free > 0
                && let Pfrw1::Astats(buf, i) = &mut w.pfrw_1
            {
                let mut as_ = PfrAstats::default();

                pfr_copyout_addr(&mut as_.pfras_a, Some(ke));

                if let Some(c) = ke.pfrke_counters.get() {
                    for d in 0..PFR_DIR_MAX {
                        for o in 0..PFR_OP_ADDR_MAX {
                            as_.pfras_packets[d][o] = c.pfrkc_packets[d][o].get();
                            as_.pfras_bytes[d][o] = c.pfrkc_bytes[d][o].get();
                        }
                    }
                } else {
                    as_.pfras_packets = [[0; PFR_OP_ADDR_MAX]; PFR_DIR_MAX];
                    as_.pfras_bytes = [[0; PFR_OP_ADDR_MAX]; PFR_DIR_MAX];
                    as_.pfras_a.pfra_fback = PFR_FB_NOCOUNT;
                }
                as_.pfras_tzero = ke.pfrke_tzero.get();

                if pfr_copyout(&as_, buf, *i, flags).is_err() {
                    return Err(Errno::EFAULT);
                }
                *i += 1;
            }
        }
        PfrwOp::PoolGet => {
            if ke.pfrke_flags.get() & PFRKE_FLAG_NOT != 0 {
                return Ok(()); // negative entries are ignored
            }
            let cnt = w.pfrw_free;
            w.pfrw_free -= 1;
            if cnt == 0 {
                w.pfrw_1 = Pfrw1::Kentry(Some(ke));
                return Err(Errno::EPERM); // finish search (the C's 1)
            }
        }
        PfrwOp::DynaddrUpdate => {
            if let Pfrw1::Dyn(dyn_) = w.pfrw_1 {
                match ke.pfrke_af.get() {
                    AF_INET => {
                        let n = dyn_.pfid_acnt4.get();
                        dyn_.pfid_acnt4.set(n + 1);
                        if n > 0 {
                            return Ok(());
                        }
                        pfr_prepare_network(&mut mask, AF_INET, i32::from(ke.pfrke_net.get()));
                        dyn_.pfid_addr4.set(sunion2pf(&ke.pfrke_sa.get(), AF_INET));
                        dyn_.pfid_mask4.set(sunion2pf(&mask, AF_INET));
                    }
                    #[cfg(feature = "inet6")]
                    AF_INET6 => {
                        let n = dyn_.pfid_acnt6.get();
                        dyn_.pfid_acnt6.set(n + 1);
                        if n > 0 {
                            return Ok(());
                        }
                        pfr_prepare_network(&mut mask, AF_INET6, i32::from(ke.pfrke_net.get()));
                        dyn_.pfid_addr6.set(sunion2pf(&ke.pfrke_sa.get(), AF_INET6));
                        dyn_.pfid_mask6.set(sunion2pf(&mask, AF_INET6));
                    }
                    af => unhandled_af(i32::from(af)),
                }
            }
        }
    }
    Ok(())
}

/// `pfr_clr_tables`: deactivates the tables `filter` selects (its anchor, or every ruleset
/// with `PFR_FLAG_ALLRSETS`), but those of the reserved anchor; the count in `ndel`.
pub fn pfr_clr_tables(
    filter: &mut PfrTable,
    ndel: Option<&mut i32>,
    flags: i32,
) -> Result<(), Errno> {
    let workq = SlistHead::<PfrKtableworkq>::new();
    let mut xdel = 0;

    accept_flags(flags, PFR_FLAG_DUMMY | PFR_FLAG_ALLRSETS)?;
    pfr_fix_anchor(&mut filter.pfrt_anchor)?;
    if pfr_table_count(filter, flags) < 0 {
        return Err(Errno::ENOENT);
    }

    for p in PFR_KTABLES.iter() {
        if pfr_skip_table(filter, p, flags) {
            continue;
        }
        if pf_cstr(p.pfrkt_anchor()) == PF_RESERVED_ANCHOR {
            continue;
        }
        if p.pfrkt_flags().get() & PFR_TFLAG_ACTIVE == 0 {
            continue;
        }
        p.pfrkt_nflags
            .set((p.pfrkt_flags().get() & !PFR_TFLAG_ACTIVE) as i32);
        // SAFETY: a work queue of this call; each table is visited once.
        unsafe { workq.insert_head(p) };
        xdel += 1;
    }
    if flags & PFR_FLAG_DUMMY == 0 {
        pfr_setflags_ktables(&workq);
    }
    if let Some(ndel) = ndel {
        *ndel = xdel;
    }
    Ok(())
}

/// `pfr_add_tables`: creates (or reactivates) the `size` tables of `tbl`, and the root tables
/// of anchored ones; the count of new tables in `nadd`.
pub fn pfr_add_tables(
    tbl: &PfrBuf<'_, PfrTable>,
    size: i32,
    nadd: Option<&mut i32>,
    flags: i32,
) -> Result<(), Errno> {
    let addq = SlistHead::<PfrKtableworkq>::new();
    let changeq = SlistHead::<PfrKtableworkq>::new();
    let auxq = SlistHead::<PfrKtableworkq>::new();
    let mut key = pf_abi_zeroed::<PfrTable>();
    let mut xadd = 0;
    let tzero = gettime();
    let user = flags & PFR_FLAG_USERIOCTL != 0;

    accept_flags(flags, PFR_FLAG_DUMMY)?;
    let rv: Result<(), Errno> = 'bad: {
        // pre-allocate all memory outside of locks
        for i in 0..size.max(0) as usize {
            pfr_yield(user);
            if pfr_copyin(tbl, i, &mut key, flags).is_err() {
                break 'bad Err(Errno::EFAULT);
            }
            if pfr_validate_table(&mut key, PFR_TFLAG_USRMASK, user).is_err() {
                break 'bad Err(Errno::EINVAL);
            }
            key.pfrt_flags.set(key.pfrt_flags.get() | PFR_TFLAG_ACTIVE);
            let Some(p) = pfr_create_ktable(&key, tzero, false, pfr_wait(flags)) else {
                break 'bad Err(Errno::ENOMEM);
            };

            // Note: we also pre-allocate a root table here. We keep it at ->pfrkt_root,
            // which we must not forget about.
            key.pfrt_flags.set(0);
            key.pfrt_anchor.fill(0);
            p.pfrkt_root
                .set(pfr_create_ktable(&key, 0, false, pfr_wait(flags)));
            if p.pfrkt_root.get().is_none() {
                pfr_destroy_ktable(p, false);
                break 'bad Err(Errno::ENOMEM);
            }

            if sl_iter(&auxq).any(|q| pfr_ktable_compare(p, q) == Ordering::Equal) {
                // We need no lock here, because `p` is empty, there are no rules or shadow
                // tables attached.
                if let Some(root) = p.pfrkt_root.get() {
                    pfr_destroy_ktable(root, false);
                }
                p.pfrkt_root.set(None);
                pfr_destroy_ktable(p, false);
                continue;
            }

            // SAFETY: a fresh table, on no queue.
            unsafe { auxq.insert_head(p) };
        }

        // auxq contains freshly allocated tables with no dups. also note there are no
        // rulesets attached, because the attach operation requires PF_LOCK().
        net_lock();
        pf_lock();
        for n in sl_iter(&auxq) {
            match PFR_KTABLES.find(n) {
                None => {
                    // SAFETY: `n` is on `auxq`; it goes onto `addq` through the same link.
                    unsafe {
                        auxq.remove(n);
                        addq.insert_head(n);
                    }
                    xadd += 1;
                }
                Some(p) => {
                    if flags & PFR_FLAG_DUMMY == 0 && p.pfrkt_flags().get() & PFR_TFLAG_ACTIVE == 0
                    {
                        p.pfrkt_nflags.set(
                            ((p.pfrkt_flags().get() & !PFR_TFLAG_USRMASK)
                                | (n.pfrkt_flags().get() & PFR_TFLAG_USRMASK)
                                | PFR_TFLAG_ACTIVE) as i32,
                        );
                        // SAFETY: a work queue of this call; an existing table is found once
                        // (the new ones have no duplicates).
                        unsafe { changeq.insert_head(p) };
                    }
                }
            }
        }

        if flags & PFR_FLAG_DUMMY == 0 {
            // addq contains tables we have to insert and attach rules to them
            //
            // changeq contains tables we need to update
            //
            // auxq contains pre-allocated tables, we won't use and we must free them
            for p in sl_iter(&addq) {
                p.pfrkt_rs.set(pf_find_or_create_ruleset(p.pfrkt_anchor()));
                let Some(rs) = p.pfrkt_rs.get() else {
                    xadd -= 1;
                    // SAFETY: `p` is on `addq`; it moves to `auxq` through the same link.
                    unsafe {
                        addq.remove(p);
                        auxq.insert_head(p);
                    }
                    continue;
                };
                rs.tables.set(rs.tables.get() + 1);

                let Some(q) = p.pfrkt_root.get() else {
                    continue;
                };
                if p.pfrkt_anchor()[0] == 0 {
                    p.pfrkt_root.set(None);
                    // SAFETY: the pre-allocated root is on no queue.
                    unsafe { auxq.insert_head(q) };
                    continue;
                }

                // use pre-allocated root table as a key
                p.pfrkt_root.set(None);
                if let Some(r) = PFR_KTABLES.find(q) {
                    p.pfrkt_root.set(Some(r));
                    // SAFETY: as above.
                    unsafe { auxq.insert_head(q) };
                    continue;
                }
                // there is a chance we could create root table in earlier iteration. such
                // table may exist in addq only then.
                if let Some(r) =
                    sl_iter(&addq).find(|r| pfr_ktable_compare(r, q) == Ordering::Equal)
                {
                    // `r` is our root table we've found earlier, `q` can get dropped.
                    p.pfrkt_root.set(Some(r));
                    // SAFETY: as above.
                    unsafe { auxq.insert_head(q) };
                    continue;
                }

                q.pfrkt_rs.set(pf_find_or_create_ruleset(q.pfrkt_anchor()));
                // root tables are attached to main ruleset, because ->pfrkt_anchor[0] == '\0'
                crate::kassert!(
                    q.pfrkt_rs
                        .get()
                        .is_some_and(|rs| ptr::eq(rs, pf_main_ruleset()))
                );
                if let Some(rs) = q.pfrkt_rs.get() {
                    rs.tables.set(rs.tables.get() + 1);
                }
                p.pfrkt_root.set(Some(q));
                // SAFETY: the root is on no queue; inserted at the head, behind the walk.
                unsafe { addq.insert_head(q) };
            }

            pfr_insert_ktables(&addq);
            pfr_setflags_ktables(&changeq);
        }
        pf_unlock();
        net_unlock();

        pfr_destroy_ktables_aux(&auxq);
        if flags & PFR_FLAG_DUMMY != 0 {
            pfr_destroy_ktables_aux(&addq);
        }

        if let Some(nadd) = nadd {
            *nadd = xadd;
        }
        return Ok(());
    };
    pfr_destroy_ktables_aux(&auxq);
    rv
}

/// `pfr_del_tables`: deactivates the `size` tables of `tbl`; the count in `ndel`.
pub fn pfr_del_tables(
    tbl: &PfrBuf<'_, PfrTable>,
    size: i32,
    ndel: Option<&mut i32>,
    flags: i32,
) -> Result<(), Errno> {
    let workq = SlistHead::<PfrKtableworkq>::new();
    let mut key = pf_abi_zeroed::<PfrTable>();
    let mut xdel = 0;
    let user = flags & PFR_FLAG_USERIOCTL != 0;

    accept_flags(flags, PFR_FLAG_DUMMY)?;
    for i in 0..size.max(0) as usize {
        pfr_yield(user);
        if pfr_copyin(tbl, i, &mut key, flags).is_err() {
            return Err(Errno::EFAULT);
        }
        pfr_validate_table(&mut key, 0, user)?;
        if let Some(p) = pfr_lookup_table(&key)
            && p.pfrkt_flags().get() & PFR_TFLAG_ACTIVE != 0
        {
            if sl_iter(&workq).any(|q| pfr_ktable_compare(p, q) == Ordering::Equal) {
                continue; // _skip
            }
            p.pfrkt_nflags
                .set((p.pfrkt_flags().get() & !PFR_TFLAG_ACTIVE) as i32);
            // SAFETY: a work queue of this call; duplicates were skipped.
            unsafe { workq.insert_head(p) };
            xdel += 1;
        }
    }

    if flags & PFR_FLAG_DUMMY == 0 {
        pfr_setflags_ktables(&workq);
    }
    if let Some(ndel) = ndel {
        *ndel = xdel;
    }
    Ok(())
}

/// `pfr_get_tables`: copies the tables `filter` selects out to `tbl`, when `*size` leaves room
/// for them; `*size` is set to their number.
pub fn pfr_get_tables(
    filter: &mut PfrTable,
    tbl: &mut PfrBuf<'_, PfrTable>,
    size: &mut i32,
    flags: i32,
) -> Result<(), Errno> {
    accept_flags(flags, PFR_FLAG_ALLRSETS)?;
    pfr_fix_anchor(&mut filter.pfrt_anchor)?;
    let nn = pfr_table_count(filter, flags);
    let mut n = nn;
    if n < 0 {
        return Err(Errno::ENOENT);
    }
    if n > *size {
        *size = n;
        return Ok(());
    }
    let mut i = 0;
    for p in PFR_KTABLES.iter() {
        if pfr_skip_table(filter, p, flags) {
            continue;
        }
        let left = n;
        n -= 1;
        if left <= 0 {
            continue;
        }
        if pfr_copyout(p.pfrkt_t(), tbl, i, flags).is_err() {
            return Err(Errno::EFAULT);
        }
        i += 1;
    }
    if n != 0 {
        crate::dpfprintf!(LOG_ERR, "pfr_get_tables: corruption detected ({}).", n);
        return Err(Errno::ENOTTY);
    }
    *size = nn;
    Ok(())
}

/// `pfr_get_tstats`: copies the tables `filter` selects, with their counters, out to `tbl`,
/// when `*size` leaves room for them; `*size` is set to their number.
pub fn pfr_get_tstats(
    filter: &mut PfrTable,
    tbl: &mut PfrBuf<'_, PfrTstats>,
    size: &mut i32,
    flags: i32,
) -> Result<(), Errno> {
    let workq = SlistHead::<PfrKtableworkq>::new();
    let tzero = gettime();

    // XXX PFR_FLAG_CLSTATS disabled
    accept_flags(flags, PFR_FLAG_ALLRSETS)?;
    pfr_fix_anchor(&mut filter.pfrt_anchor)?;
    let nn = pfr_table_count(filter, flags);
    let mut n = nn;
    if n < 0 {
        return Err(Errno::ENOENT);
    }
    if n > *size {
        *size = n;
        return Ok(());
    }
    let mut i = 0;
    for p in PFR_KTABLES.iter() {
        if pfr_skip_table(filter, p, flags) {
            continue;
        }
        let left = n;
        n -= 1;
        if left <= 0 {
            continue;
        }
        if pfr_copyout(&p.pfrkt_ts, tbl, i, flags).is_err() {
            return Err(Errno::EFAULT);
        }
        i += 1;
        // SAFETY: a work queue of this call; each table is visited once.
        unsafe { workq.insert_head(p) };
    }
    if flags & PFR_FLAG_CLSTATS != 0 {
        pfr_clstats_ktables(&workq, tzero, flags & PFR_FLAG_ADDRSTOO != 0);
    }
    if n != 0 {
        crate::dpfprintf!(LOG_ERR, "pfr_get_tstats: corruption detected ({}).", n);
        return Err(Errno::ENOTTY);
    }
    *size = nn;
    Ok(())
}

/// `pfr_clr_tstats`: clears the counters of the `size` tables of `tbl` (and of their
/// addresses with `PFR_FLAG_ADDRSTOO`); the count in `nzero`.
pub fn pfr_clr_tstats(
    tbl: &PfrBuf<'_, PfrTable>,
    size: i32,
    nzero: Option<&mut i32>,
    flags: i32,
) -> Result<(), Errno> {
    let workq = SlistHead::<PfrKtableworkq>::new();
    let mut key = pf_abi_zeroed::<PfrTable>();
    let mut xzero = 0;
    let tzero = gettime();

    accept_flags(flags, PFR_FLAG_DUMMY | PFR_FLAG_ADDRSTOO)?;
    for i in 0..size.max(0) as usize {
        pfr_yield(flags & PFR_FLAG_USERIOCTL != 0);
        if pfr_copyin(tbl, i, &mut key, flags).is_err() {
            return Err(Errno::EFAULT);
        }
        pfr_validate_table(&mut key, 0, false)?;
        if let Some(p) = pfr_lookup_table(&key) {
            // SAFETY: a work queue of this call (a table given twice is queued twice, as in
            // C).
            unsafe { workq.insert_head(p) };
            xzero += 1;
        }
    }
    if flags & PFR_FLAG_DUMMY == 0 {
        pfr_clstats_ktables(&workq, tzero, flags & PFR_FLAG_ADDRSTOO != 0);
    }
    if let Some(nzero) = nzero {
        *nzero = xzero;
    }
    Ok(())
}

/// `pfr_set_tflags`: sets `setflag` and clears `clrflag` (user flags) on the `size` active
/// tables of `tbl`; the counts of changed and deleted (no longer persistent) tables in
/// `nchange` and `ndel`.
pub fn pfr_set_tflags(
    tbl: &PfrBuf<'_, PfrTable>,
    size: i32,
    setflag: i32,
    clrflag: i32,
    nchange: Option<&mut i32>,
    ndel: Option<&mut i32>,
    flags: i32,
) -> Result<(), Errno> {
    let workq = SlistHead::<PfrKtableworkq>::new();
    let mut key = pf_abi_zeroed::<PfrTable>();
    let (mut xchange, mut xdel) = (0, 0);
    let user = flags & PFR_FLAG_USERIOCTL != 0;
    let (setflag, clrflag) = (setflag as u32, clrflag as u32);

    accept_flags(flags, PFR_FLAG_DUMMY)?;
    if (setflag & !PFR_TFLAG_USRMASK) != 0
        || (clrflag & !PFR_TFLAG_USRMASK) != 0
        || (setflag & clrflag) != 0
    {
        return Err(Errno::EINVAL);
    }
    for i in 0..size.max(0) as usize {
        pfr_yield(user);
        if pfr_copyin(tbl, i, &mut key, flags).is_err() {
            return Err(Errno::EFAULT);
        }
        pfr_validate_table(&mut key, 0, user)?;
        if let Some(p) = pfr_lookup_table(&key)
            && p.pfrkt_flags().get() & PFR_TFLAG_ACTIVE != 0
        {
            p.pfrkt_nflags
                .set(((p.pfrkt_flags().get() | setflag) & !clrflag) as i32);
            if p.pfrkt_nflags.get() as u32 == p.pfrkt_flags().get() {
                continue; // _skip
            }
            if sl_iter(&workq).any(|q| pfr_ktable_compare(p, q) == Ordering::Equal) {
                continue; // _skip
            }
            // SAFETY: a work queue of this call; duplicates were skipped.
            unsafe { workq.insert_head(p) };
            if p.pfrkt_flags().get() & PFR_TFLAG_PERSIST != 0
                && clrflag & PFR_TFLAG_PERSIST != 0
                && p.pfrkt_flags().get() & PFR_TFLAG_REFERENCED == 0
            {
                xdel += 1;
            } else {
                xchange += 1;
            }
        }
    }
    if flags & PFR_FLAG_DUMMY == 0 {
        pfr_setflags_ktables(&workq);
    }
    if let Some(nchange) = nchange {
        *nchange = xchange;
    }
    if let Some(ndel) = ndel {
        *ndel = xdel;
    }
    Ok(())
}

/// `pfr_ina_begin`: opens a table transaction on the anchor of `trs`, dropping the inactive
/// tables a previous one left; the new ticket in `ticket`, the count of dropped tables in
/// `ndel`.
pub fn pfr_ina_begin(
    trs: &PfrTable,
    ticket: Option<&mut u32>,
    ndel: Option<&mut i32>,
    flags: i32,
) -> Result<(), Errno> {
    let workq = SlistHead::<PfrKtableworkq>::new();
    let mut xdel = 0;

    accept_flags(flags, PFR_FLAG_DUMMY)?;
    let rs = pf_find_or_create_ruleset(&trs.pfrt_anchor).ok_or(Errno::ENOMEM)?;
    for p in PFR_KTABLES.iter() {
        if p.pfrkt_flags().get() & PFR_TFLAG_INACTIVE == 0 || pfr_skip_table(trs, p, 0) {
            continue;
        }
        p.pfrkt_nflags
            .set((p.pfrkt_flags().get() & !PFR_TFLAG_INACTIVE) as i32);
        // SAFETY: a work queue of this call; each table is visited once.
        unsafe { workq.insert_head(p) };
        xdel += 1;
    }
    if flags & PFR_FLAG_DUMMY == 0 {
        pfr_setflags_ktables(&workq);
        rs.tticket.set(rs.tticket.get().wrapping_add(1));
        if let Some(ticket) = ticket {
            *ticket = rs.tticket.get();
        }
        rs.topen.set(1);
    } else {
        pf_remove_if_empty_ruleset(rs);
    }
    if let Some(ndel) = ndel {
        *ndel = xdel;
    }
    Ok(())
}

/// `pfr_ina_define`: defines the table `tbl` in the open transaction `ticket`, with the
/// `size` addresses of `addr` (`PFR_FLAG_ADDRSTOO`) in its shadow; the counts of new tables and
/// of addresses in `nadd` and `naddr`.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn pfr_ina_define(
    tbl: &mut PfrTable,
    addr: &PfrBuf<'_, PfrAddr>,
    size: i32,
    nadd: Option<&mut i32>,
    naddr: Option<&mut i32>,
    ticket: u32,
    flags: i32,
) -> Result<(), Errno> {
    let tableq = SlistHead::<PfrKtableworkq>::new();
    let addrq = SlistHead::<PfrKentryworkq>::new();
    let mut ad = PfrAddr::default();
    let (mut xadd, mut xaddr) = (0, 0);
    let user = flags & PFR_FLAG_USERIOCTL != 0;

    accept_flags(flags, PFR_FLAG_DUMMY | PFR_FLAG_ADDRSTOO)?;
    if size != 0 && flags & PFR_FLAG_ADDRSTOO == 0 {
        return Err(Errno::EINVAL);
    }
    pfr_validate_table(tbl, PFR_TFLAG_USRMASK, user)?;
    let rs = pf_find_ruleset(&tbl.pfrt_anchor);
    if !rs.is_some_and(|rs| rs.topen.get() != 0 && ticket == rs.tticket.get()) {
        return Err(Errno::EBUSY);
    }
    tbl.pfrt_flags
        .set(tbl.pfrt_flags.get() | PFR_TFLAG_INACTIVE);
    let kt = match pfr_lookup_table(tbl) {
        None => {
            let kt = pfr_create_ktable(tbl, 0, true, pfr_wait(flags)).ok_or(Errno::ENOMEM)?;
            // SAFETY: a fresh table, on no queue.
            unsafe { tableq.insert_head(kt) };
            xadd += 1;
            'skip: {
                if tbl.pfrt_anchor[0] == 0 {
                    break 'skip;
                }

                // find or create root table
                let mut key = pf_abi_zeroed::<PfrTable>();
                strlcpy(&mut key.pfrt_name, &tbl.pfrt_name);
                if let Some(rt) = pfr_lookup_table(&key) {
                    kt.pfrkt_root.set(Some(rt));
                    break 'skip;
                }
                let Some(rt) = pfr_create_ktable(&key, 0, true, pfr_wait(flags)) else {
                    pfr_destroy_ktables(&tableq, false);
                    return Err(Errno::ENOMEM);
                };
                // SAFETY: a fresh table, on no queue.
                unsafe { tableq.insert_head(rt) };
                kt.pfrkt_root.set(Some(rt));
            }
            kt
        }
        Some(kt) => {
            if kt.pfrkt_flags().get() & PFR_TFLAG_INACTIVE == 0 {
                xadd += 1;
            }
            kt
        }
    };
    // _skip:
    let Some(shadow) = pfr_create_ktable(tbl, 0, false, pfr_wait(flags)) else {
        pfr_destroy_ktables(&tableq, false);
        return Err(Errno::ENOMEM);
    };
    let rv: Result<(), Errno> = 'bad: {
        for i in 0..size.max(0) as usize {
            pfr_yield(user);
            if pfr_copyin(addr, i, &mut ad, flags).is_err() {
                break 'bad Err(Errno::EFAULT);
            }
            if !pfr_validate_addr(&ad) {
                break 'bad Err(Errno::EINVAL);
            }
            if pfr_lookup_addr(shadow, &ad, true).is_some() {
                continue;
            }
            let Some(p) = pfr_create_kentry(&mut ad) else {
                break 'bad Err(Errno::ENOMEM);
            };
            if !pfr_route_kentry(shadow, p) {
                pfr_destroy_kentry(p);
                continue;
            }
            // SAFETY: a fresh entry, on no queue.
            unsafe { addrq.insert_head(p) };
            xaddr += 1;
            if p.pfrke_type.get() == PFRKE_COST {
                kt.pfrkt_refcntcost.set(kt.pfrkt_refcntcost.get() + 1);
            }
            pfr_ktable_winfo_update(kt, p);
        }
        if flags & PFR_FLAG_DUMMY == 0 {
            if let Some(old) = kt.pfrkt_shadow.get() {
                pfr_destroy_ktable(old, true);
            }
            kt.pfrkt_flags()
                .set(kt.pfrkt_flags().get() | PFR_TFLAG_INACTIVE);
            pfr_insert_ktables(&tableq);
            shadow.pfrkt_cnt().set(if flags & PFR_FLAG_ADDRSTOO != 0 {
                xaddr
            } else {
                NO_ADDRESSES
            });
            kt.pfrkt_shadow.set(Some(shadow));
        } else {
            pfr_clean_node_mask(shadow, &addrq);
            pfr_destroy_ktable(shadow, false);
            pfr_destroy_ktables(&tableq, false);
            pfr_destroy_kentries(&addrq);
        }
        if let Some(nadd) = nadd {
            *nadd = xadd;
        }
        if let Some(naddr) = naddr {
            *naddr = xaddr;
        }
        return Ok(());
    };
    pfr_destroy_ktable(shadow, false);
    pfr_destroy_ktables(&tableq, false);
    pfr_destroy_kentries(&addrq);
    rv
}

/// `pfr_ina_rollback`: drops the inactive tables of the transaction `ticket` on the anchor of
/// `trs` and closes it; the count in `ndel`. A stale ticket is not an error.
pub fn pfr_ina_rollback(
    trs: &PfrTable,
    ticket: u32,
    ndel: Option<&mut i32>,
    flags: i32,
) -> Result<(), Errno> {
    let workq = SlistHead::<PfrKtableworkq>::new();
    let mut xdel = 0;

    accept_flags(flags, PFR_FLAG_DUMMY)?;
    let rs = pf_find_ruleset(&trs.pfrt_anchor);
    let Some(rs) = rs.filter(|rs| rs.topen.get() != 0 && ticket == rs.tticket.get()) else {
        return Ok(());
    };
    for p in PFR_KTABLES.iter() {
        if p.pfrkt_flags().get() & PFR_TFLAG_INACTIVE == 0 || pfr_skip_table(trs, p, 0) {
            continue;
        }
        p.pfrkt_nflags
            .set((p.pfrkt_flags().get() & !PFR_TFLAG_INACTIVE) as i32);
        // SAFETY: a work queue of this call; each table is visited once.
        unsafe { workq.insert_head(p) };
        xdel += 1;
    }
    if flags & PFR_FLAG_DUMMY == 0 {
        pfr_setflags_ktables(&workq);
        rs.topen.set(0);
        pf_remove_if_empty_ruleset(rs);
    }
    if let Some(ndel) = ndel {
        *ndel = xdel;
    }
    Ok(())
}

/// `pfr_ina_commit`: makes the inactive tables of the transaction `ticket` on the anchor of
/// `trs` active and closes it; the counts of new and changed tables in `nadd` and `nchange`.
pub fn pfr_ina_commit(
    trs: &PfrTable,
    ticket: u32,
    nadd: Option<&mut i32>,
    nchange: Option<&mut i32>,
    flags: i32,
) -> Result<(), Errno> {
    let workq = SlistHead::<PfrKtableworkq>::new();
    let (mut xadd, mut xchange) = (0, 0);
    let tzero = gettime();

    accept_flags(flags, PFR_FLAG_DUMMY)?;
    let rs = pf_find_ruleset(&trs.pfrt_anchor);
    let Some(rs) = rs.filter(|rs| rs.topen.get() != 0 && ticket == rs.tticket.get()) else {
        return Err(Errno::EBUSY);
    };

    for p in PFR_KTABLES.iter() {
        if p.pfrkt_flags().get() & PFR_TFLAG_INACTIVE == 0 || pfr_skip_table(trs, p, 0) {
            continue;
        }
        // SAFETY: a work queue of this call; each table is visited once.
        unsafe { workq.insert_head(p) };
        if p.pfrkt_flags().get() & PFR_TFLAG_ACTIVE != 0 {
            xchange += 1;
        } else {
            xadd += 1;
        }
    }

    if flags & PFR_FLAG_DUMMY == 0 {
        for p in sl_iter(&workq) {
            pfr_commit_ktable(p, tzero);
        }
        rs.topen.set(0);
        pf_remove_if_empty_ruleset(rs);
    }
    if let Some(nadd) = nadd {
        *nadd = xadd;
    }
    if let Some(nchange) = nchange {
        *nchange = xchange;
    }

    Ok(())
}

/// `pfr_commit_ktable`: makes the shadow of `kt` its content: merged address by address when
/// `kt` is active (it might contain addresses), swapped in otherwise.
pub fn pfr_commit_ktable(kt: &'static PfrKtable, tzero: Time) {
    let Some(shadow) = kt.pfrkt_shadow.get() else {
        panic(format_args!("pfr_commit_ktable: no shadow"));
    };

    if shadow.pfrkt_cnt().get() == NO_ADDRESSES {
        if kt.pfrkt_flags().get() & PFR_TFLAG_ACTIVE == 0 {
            pfr_clstats_ktable(kt, tzero, true);
        }
    } else if kt.pfrkt_flags().get() & PFR_TFLAG_ACTIVE != 0 {
        // kt might contain addresses
        let addrq = SlistHead::<PfrKentryworkq>::new();
        let addq = SlistHead::<PfrKentryworkq>::new();
        let changeq = SlistHead::<PfrKentryworkq>::new();
        let delq = SlistHead::<PfrKentryworkq>::new();
        let garbageq = SlistHead::<PfrKentryworkq>::new();
        let mut ad = PfrAddr::default();

        pfr_enqueue_addrs(shadow, &addrq, None, false);
        pfr_mark_addrs(kt);
        pfr_clean_node_mask(shadow, &addrq);
        while let Some(p) = sl_first(&addrq) {
            // SAFETY: the queue holds `p` at its head.
            unsafe { addrq.remove_head() };
            pfr_copyout_addr(&mut ad, Some(p));
            if let Some(q) = pfr_lookup_addr(kt, &ad, true) {
                if (q.pfrke_flags.get() & PFRKE_FLAG_NOT) != (p.pfrke_flags.get() & PFRKE_FLAG_NOT)
                {
                    // SAFETY: a work queue of this call; the mark below keeps `q` from being
                    // queued twice.
                    unsafe { changeq.insert_head(q) };
                }
                q.pfrke_flags.set(q.pfrke_flags.get() | PFRKE_FLAG_MARK);
                // SAFETY: `p` just left `addrq`.
                unsafe { garbageq.insert_head(p) };
            } else {
                p.pfrke_tzero.set(tzero);
                // SAFETY: `p` just left `addrq`.
                unsafe { addq.insert_head(p) };
            }
        }
        pfr_enqueue_addrs(kt, &delq, None, ENQUEUE_UNMARKED_ONLY);
        pfr_insert_kentries(kt, &addq, tzero);
        pfr_remove_kentries(kt, &delq);
        pfr_clstats_kentries(&changeq, tzero, INVERT_NEG_FLAG);
        pfr_destroy_kentries(&garbageq);
    } else {
        // kt cannot contain addresses
        kt.pfrkt_ip4.swap(&shadow.pfrkt_ip4);
        kt.pfrkt_ip6.swap(&shadow.pfrkt_ip6);
        kt.pfrkt_cnt().swap(shadow.pfrkt_cnt());
        pfr_clstats_ktable(kt, tzero, true);
    }
    let nflags = ((shadow.pfrkt_flags().get() & PFR_TFLAG_USRMASK)
        | (kt.pfrkt_flags().get() & PFR_TFLAG_SETMASK)
        | PFR_TFLAG_ACTIVE)
        & !PFR_TFLAG_INACTIVE;
    pfr_destroy_ktable(shadow, false);
    kt.pfrkt_shadow.set(None);
    pfr_setflags_ktable(kt, nflags);
}

/// `pfr_validate_table`: checks the name, anchor (rewritten by `pfr_fix_anchor`) and flags of
/// `tbl`; `no_reserved` refuses the reserved anchor.
pub fn pfr_validate_table(
    tbl: &mut PfrTable,
    allowedflags: u32,
    no_reserved: bool,
) -> Result<(), Errno> {
    if tbl.pfrt_name[0] == 0 {
        return Err(Errno::EINVAL);
    }
    if no_reserved && pf_cstr(&tbl.pfrt_anchor) == PF_RESERVED_ANCHOR {
        return Err(Errno::EINVAL);
    }
    if strnlen(&tbl.pfrt_name, PF_TABLE_NAME_SIZE) >= PF_TABLE_NAME_SIZE {
        return Err(Errno::ENAMETOOLONG);
    }
    pfr_fix_anchor(&mut tbl.pfrt_anchor)?;
    if tbl.pfrt_flags.get() & !allowedflags != 0 {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

/// `pfr_fix_anchor`: rewrite anchors referenced by tables to remove slashes and check for
/// validity.
pub fn pfr_fix_anchor(anchor: &mut [u8; PATH_MAX]) -> Result<(), Errno> {
    let siz = PATH_MAX;

    if strnlen(anchor, PATH_MAX) >= PATH_MAX {
        return Err(Errno::ENAMETOOLONG);
    }
    if anchor[0] == b'/' {
        let mut off = 1;
        while anchor[off] == b'/' {
            off += 1;
        }
        anchor.copy_within(off.., 0);
        anchor[siz - off..].fill(0);
    }
    Ok(())
}

/// `pfr_table_count`: the number of tables `filter` selects, -1 for an unknown anchor.
pub fn pfr_table_count(filter: &PfrTable, flags: i32) -> i32 {
    if flags & PFR_FLAG_ALLRSETS != 0 {
        return PFR_KTABLE_CNT.get();
    }
    if filter.pfrt_anchor[0] != 0 {
        let rs = pf_find_ruleset(&filter.pfrt_anchor);
        return rs.map_or(-1, |rs| rs.tables.get());
    }
    pf_main_ruleset().tables.get()
}

/// `pfr_skip_table`: `true` when `filter` does not select `kt` (another anchor, without
/// `PFR_FLAG_ALLRSETS`).
pub fn pfr_skip_table(filter: &PfrTable, kt: &PfrKtable, flags: i32) -> bool {
    if flags & PFR_FLAG_ALLRSETS != 0 {
        return false;
    }
    pf_cstr(&filter.pfrt_anchor) != pf_cstr(kt.pfrkt_anchor())
}

/// `pfr_insert_ktables`: inserts the tables of `workq` into `pfr_ktables`.
pub fn pfr_insert_ktables(workq: &SlistHead<PfrKtableworkq>) {
    for p in sl_iter(workq) {
        pfr_insert_ktable(p);
    }
}

/// `pfr_insert_ktable`: inserts `kt` into `pfr_ktables`, referencing its root table.
pub fn pfr_insert_ktable(kt: &'static PfrKtable) {
    // SAFETY: `kt` is a pool item in no tree; it stays allocated until removed again
    // (`pfr_setflags_ktable`).
    let _ = unsafe { PFR_KTABLES.insert(kt) };
    PFR_KTABLE_CNT.set(PFR_KTABLE_CNT.get() + 1);
    if let Some(root) = kt.pfrkt_root.get() {
        let r = &root.pfrkt_refcnt()[PFR_REFCNT_ANCHOR];
        let old = r.get();
        r.set(old + 1);
        if old == 0 {
            pfr_setflags_ktable(root, root.pfrkt_flags().get() | PFR_TFLAG_REFDANCHOR);
        }
    }
}

/// `pfr_setflags_ktables`: gives each table of `workq` its `pfrkt_nflags`.
pub fn pfr_setflags_ktables(workq: &SlistHead<PfrKtableworkq>) {
    for p in sl_iter(workq) {
        pfr_setflags_ktable(p, p.pfrkt_nflags.get() as u32);
    }
}

/// `pfr_setflags_ktable`: gives `kt` the flags `newf`, destroying it when it is no longer
/// referenced, persistent, active or inactive, its addresses when it is no longer active, and
/// its shadow when it is no longer inactive.
pub fn pfr_setflags_ktable(kt: &'static PfrKtable, mut newf: u32) {
    let addrq = SlistHead::<PfrKentryworkq>::new();

    if newf & PFR_TFLAG_REFERENCED == 0
        && newf & PFR_TFLAG_REFDANCHOR == 0
        && newf & PFR_TFLAG_PERSIST == 0
    {
        newf &= !PFR_TFLAG_ACTIVE;
    }
    if newf & PFR_TFLAG_ACTIVE == 0 {
        newf &= !PFR_TFLAG_USRMASK;
    }
    if newf & PFR_TFLAG_SETMASK == 0 {
        // SAFETY: a table with flags in `PFR_TFLAG_SETMASK` is in `pfr_ktables`.
        unsafe { PFR_KTABLES.remove(kt) };
        if let Some(root) = kt.pfrkt_root.get() {
            let r = &root.pfrkt_refcnt()[PFR_REFCNT_ANCHOR];
            r.set(r.get() - 1);
            if r.get() == 0 {
                pfr_setflags_ktable(root, root.pfrkt_flags().get() & !PFR_TFLAG_REFDANCHOR);
            }
        }
        pfr_destroy_ktable(kt, true);
        PFR_KTABLE_CNT.set(PFR_KTABLE_CNT.get() - 1);
        return;
    }
    if newf & PFR_TFLAG_ACTIVE == 0 && kt.pfrkt_cnt().get() != 0 {
        pfr_enqueue_addrs(kt, &addrq, None, false);
        pfr_remove_kentries(kt, &addrq);
    }
    if newf & PFR_TFLAG_INACTIVE == 0
        && let Some(shadow) = kt.pfrkt_shadow.get()
    {
        pfr_destroy_ktable(shadow, true);
        kt.pfrkt_shadow.set(None);
    }
    kt.pfrkt_flags().set(newf);
}

/// `pfr_clstats_ktables`: clears the counters of the tables of `workq`.
pub fn pfr_clstats_ktables(workq: &SlistHead<PfrKtableworkq>, tzero: Time, recurse: bool) {
    for p in sl_iter(workq) {
        pfr_clstats_ktable(p, tzero, recurse);
    }
}

/// `pfr_clstats_ktable`: clears the counters of `kt` (and of its entries with `recurse`).
pub fn pfr_clstats_ktable(kt: &'static PfrKtable, tzero: Time, recurse: bool) {
    let addrq = SlistHead::<PfrKentryworkq>::new();

    if recurse {
        pfr_enqueue_addrs(kt, &addrq, None, false);
        pfr_clstats_kentries(&addrq, tzero, false);
    }
    for d in 0..PFR_DIR_MAX {
        for o in 0..PFR_OP_TABLE_MAX {
            kt.pfrkt_packets()[d][o].set(0);
            kt.pfrkt_bytes()[d][o].set(0);
        }
    }
    kt.pfrkt_match().set(0);
    kt.pfrkt_nomatch().set(0);
    kt.pfrkt_tzero().set(tzero);
}

/// `pfr_create_ktable`: a new table described by `tbl`, with empty radix trees, attached to
/// the ruleset of its anchor with `attachruleset` (under `pf_lock`).
pub fn pfr_create_ktable(
    tbl: &PfrTable,
    tzero: Time,
    attachruleset: bool,
    wait: i32,
) -> Option<&'static PfrKtable> {
    let p = pool_get(&PFR_KTABLE_PL, wait | PR_ZERO | PR_LIMITFAIL)?
        .cast::<PfrKtable>()
        .as_ptr();
    // SAFETY: a fresh, zeroed item of `pfr_ktable_pl`, sized and aligned for a `PfrKtable`
    // (`pfr_initialize`), and all-zero is a valid one (`PfPoolItem`). Nothing refers to it
    // yet, so the table description (`pfrkt_t`, whose name and anchor are not `Cell`s) is
    // copied in through the raw pointer before the first shared reference is made.
    let kt: &'static PfrKtable = unsafe {
        ptr::copy_nonoverlapping(
            ptr::from_ref(tbl),
            ptr::addr_of_mut!((*p).pfrkt_ts.pfrts_t),
            1,
        );
        &*p
    };

    if attachruleset {
        pf_assert_locked();
        let Some(rs) = pf_find_or_create_ruleset(&tbl.pfrt_anchor) else {
            pfr_destroy_ktable(kt, false);
            return None;
        };
        kt.pfrkt_rs.set(Some(rs));
        rs.tables.set(rs.tables.get() + 1);
    }

    let mut ip4 = kt.pfrkt_ip4.get();
    let ok = rn_inithead(&mut ip4, offset_of!(SockaddrIn, sin_addr) as i32);
    kt.pfrkt_ip4.set(ip4);
    let ok = ok && {
        let mut ip6 = kt.pfrkt_ip6.get();
        let ok = rn_inithead(&mut ip6, offset_of!(SockaddrIn6, sin6_addr) as i32);
        kt.pfrkt_ip6.set(ip6);
        ok
    };
    if !ok {
        pfr_destroy_ktable(kt, false);
        return None;
    }
    kt.pfrkt_tzero().set(tzero);
    kt.pfrkt_refcntcost.set(0);
    kt.pfrkt_gcdweight.set(0);
    kt.pfrkt_maxweight.set(1);

    Some(kt)
}

/// `pfr_destroy_ktables`: destroys the tables of `workq`, emptying it.
pub fn pfr_destroy_ktables(workq: &SlistHead<PfrKtableworkq>, flushaddr: bool) {
    while let Some(p) = sl_first(workq) {
        // SAFETY: the queue holds `p` at its head.
        unsafe { workq.remove_head() };
        pfr_destroy_ktable(p, flushaddr);
    }
}

/// `pfr_destroy_ktables_aux`: destroys the pre-allocated tables of `auxq` and their root
/// tables, emptying it.
pub fn pfr_destroy_ktables_aux(auxq: &SlistHead<PfrKtableworkq>) {
    while let Some(p) = sl_first(auxq) {
        // SAFETY: the queue holds `p` at its head.
        unsafe { auxq.remove_head() };
        // There must be no extra data (rules, shadow tables, ...) attached, because auxq
        // holds just empty memory to be initialized. Therefore we can also be called with no
        // lock.
        if let Some(root) = p.pfrkt_root.get() {
            crate::kassert!(root.pfrkt_rs.get().is_none());
            crate::kassert!(root.pfrkt_shadow.get().is_none());
            crate::kassert!(root.pfrkt_root.get().is_none());
            pfr_destroy_ktable(root, false);
            p.pfrkt_root.set(None);
        }
        crate::kassert!(p.pfrkt_rs.get().is_none());
        crate::kassert!(p.pfrkt_shadow.get().is_none());
        pfr_destroy_ktable(p, false);
    }
}

/// `pfr_destroy_ktable`: frees `kt`, its radix heads (its entries too with `flushaddr`) and
/// its shadow, and drops its ruleset's table count. `kt` is out of `pfr_ktables`.
pub fn pfr_destroy_ktable(kt: &'static PfrKtable, flushaddr: bool) {
    let addrq = SlistHead::<PfrKentryworkq>::new();

    if flushaddr {
        pfr_enqueue_addrs(kt, &addrq, None, false);
        pfr_clean_node_mask(kt, &addrq);
        pfr_destroy_kentries(&addrq);
    }
    if let Some(h) = kt.pfrkt_ip4.get() {
        free(
            NonNull::from(h).cast(),
            M_RTABLE,
            size_of::<RadixNodeHead>(),
        );
    }
    if let Some(h) = kt.pfrkt_ip6.get() {
        free(
            NonNull::from(h).cast(),
            M_RTABLE,
            size_of::<RadixNodeHead>(),
        );
    }
    if let Some(shadow) = kt.pfrkt_shadow.get() {
        pfr_destroy_ktable(shadow, flushaddr);
    }
    if let Some(rs) = kt.pfrkt_rs.get() {
        rs.tables.set(rs.tables.get() - 1);
        pf_remove_if_empty_ruleset(rs);
    }
    pf_pool_put(&PFR_KTABLE_PL, kt);
}

/// The order of two table descriptions: name (at most `PF_TABLE_NAME_SIZE` bytes), then
/// anchor.
fn pfr_table_compare(p: &PfrTable, q: &PfrTable) -> Ordering {
    pf_cstr(&p.pfrt_name)
        .cmp(pf_cstr(&q.pfrt_name))
        .then_with(|| pf_cstr(&p.pfrt_anchor).cmp(pf_cstr(&q.pfrt_anchor)))
}

/// `pfr_ktable_compare`: tables are ordered by name, then anchor.
pub fn pfr_ktable_compare(p: &PfrKtable, q: &PfrKtable) -> Ordering {
    pfr_table_compare(p.pfrkt_t(), q.pfrkt_t())
}

/// `pfr_lookup_table`: the table named and anchored as `tbl`.
pub fn pfr_lookup_table(tbl: &PfrTable) -> Option<&'static PfrKtable> {
    // struct pfr_ktable start like a struct pfr_table
    let mut n = PFR_KTABLES.root();
    while let Some(kt) = n {
        n = match pfr_table_compare(tbl, kt.pfrkt_t()) {
            Ordering::Less => RbHead::<PfrKtablehead>::left(kt),
            Ordering::Greater => RbHead::<PfrKtablehead>::right(kt),
            Ordering::Equal => return Some(kt),
        };
    }
    None
}

/// `pfr_match_addr`: whether the active table `kt` (or its root) holds `a` in a positive
/// entry; counts the match or miss.
pub fn pfr_match_addr(kt: &'static PfrKtable, a: &PfAddr, af: SaFamily) -> bool {
    let ke = pfr_kentry_byaddr(kt, a, af, false);

    let m = ke.is_some_and(|ke| ke.pfrke_flags.get() & PFRKE_FLAG_NOT == 0);
    if m {
        kt.pfrkt_match().set(kt.pfrkt_match().get().wrapping_add(1));
    } else {
        kt.pfrkt_nomatch()
            .set(kt.pfrkt_nomatch().get().wrapping_add(1));
    }

    m
}

/// The radix key of the IPv4 address in the first word of `a` (`tmp4`).
fn pfr_sin_key(a: &PfAddr) -> PfsockaddrUnion {
    let mut tmp4 = PfsockaddrUnion::default();
    fillin_sin(
        &mut tmp4,
        InAddr {
            s_addr: a.addr32(0),
        },
    );
    tmp4
}

/// The `sockaddr_in6` key (`tmp6`) of the address `a`.
#[cfg(feature = "inet6")]
fn pfr_sin6_key(a: &PfAddr) -> PfsockaddrUnion {
    let mut tmp6 = PfsockaddrUnion::default();
    fillin_sin6(&mut tmp6, a.v6());
    tmp6
}

/// `pfr_kentry_byaddr`: the entry of the active table `kt` (or its root) that best matches
/// `a` (`None` with `exact` when that is a network).
pub fn pfr_kentry_byaddr(
    kt: &'static PfrKtable,
    a: &PfAddr,
    af: SaFamily,
    exact: bool,
) -> Option<&'static PfrKentry> {
    let kt = pfr_ktable_select_active(kt)?;

    let ke = match af {
        AF_INET => {
            let tmp4 = pfr_sin_key(a);
            // SAFETY: the table's tree holds entries; the key is a 28-byte union.
            unsafe { rn_match(tmp4.bytes.as_ptr(), pfr_rnh(&kt.pfrkt_ip4)) }.map(rn2ke)
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            let tmp6 = pfr_sin6_key(a);
            // SAFETY: as for the IPv4 key.
            unsafe { rn_match(tmp6.bytes.as_ptr(), pfr_rnh(&kt.pfrkt_ip6)) }.map(rn2ke)
        }
        _ => unhandled_af(i32::from(af)),
    };
    ke.filter(|ke| !(exact && kentry_network(ke)))
}

/// `pfr_update_stats`: counts the packet `pd` against the table `kt` and the entry matching
/// `a`, as rule action `op`; `notrule` says the rule negated the table.
pub fn pfr_update_stats(kt: &'static PfrKtable, a: &PfAddr, pd: &PfPdesc, op: u8, notrule: bool) {
    let af = pd.af;
    let len = pd.tot_len;
    let dir_idx = usize::from(pd.dir == PF_OUT);

    let Some(kt) = pfr_ktable_select_active(kt) else {
        return;
    };

    let ke = match af {
        AF_INET => {
            let tmp4 = pfr_sin_key(a);
            // SAFETY: as in `pfr_kentry_byaddr`.
            unsafe { rn_match(tmp4.bytes.as_ptr(), pfr_rnh(&kt.pfrkt_ip4)) }.map(rn2ke)
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            let tmp6 = pfr_sin6_key(a);
            // SAFETY: as for the IPv4 key.
            unsafe { rn_match(tmp6.bytes.as_ptr(), pfr_rnh(&kt.pfrkt_ip6)) }.map(rn2ke)
        }
        _ => unhandled_af(i32::from(af)),
    };

    let mut op_idx = match op {
        PF_PASS => PFR_OP_PASS,
        PF_MATCH => PFR_OP_MATCH,
        PF_DROP => PFR_OP_BLOCK,
        _ => panic(format_args!("unhandled op")),
    };

    if ke.is_none_or(|ke| ke.pfrke_flags.get() & PFRKE_FLAG_NOT != 0) != notrule {
        if op_idx != PFR_OP_PASS {
            crate::dpfprintf!(LOG_DEBUG, "pfr_update_stats: assertion failed.");
        }
        op_idx = PFR_OP_XPASS;
    }
    let c = &kt.pfrkt_packets()[dir_idx][op_idx];
    c.set(c.get().wrapping_add(1));
    let c = &kt.pfrkt_bytes()[dir_idx][op_idx];
    c.set(c.get().wrapping_add(len));
    if let Some(ke) = ke
        && op_idx != PFR_OP_XPASS
        && kt.pfrkt_flags().get() & PFR_TFLAG_COUNTERS != 0
    {
        if ke.pfrke_counters.get().is_none() {
            ke.pfrke_counters.set(pf_pool_get::<PfrKcounters>(
                &PFR_KCOUNTERS_PL,
                PR_NOWAIT | PR_ZERO,
            ));
        }
        if let Some(c) = ke.pfrke_counters.get() {
            let p = &c.pfrkc_packets[dir_idx][op_idx];
            p.set(p.get().wrapping_add(1));
            let b = &c.pfrkc_bytes[dir_idx][op_idx];
            b.set(b.get().wrapping_add(len));
        }
    }
}

/// `pfr_attach_table`: the table `name` of the ruleset `rs` (created, with its root table for
/// an anchor, when missing), referenced by one more rule.
pub fn pfr_attach_table(
    rs: &'static PfRuleset,
    name: &[u8],
    wait: i32,
) -> Option<&'static PfrKtable> {
    let ac = rs.anchor.get();

    let mut tbl = pf_abi_zeroed::<PfrTable>();
    strlcpy(&mut tbl.pfrt_name, name);
    if let Some(ac) = ac {
        strlcpy(&mut tbl.pfrt_anchor, &ac.path);
    }
    let kt = match pfr_lookup_table(&tbl) {
        Some(kt) => kt,
        None => {
            // Hold rs across the table creation below. A new table takes a reference on the
            // ruleset, and pfr_destroy_ktable() drops it again on the failure paths, which
            // lets pf_remove_if_empty_ruleset() free the anchor rs points into while our
            // caller still holds rs.
            rs.tables.set(rs.tables.get() + 1);
            let Some(kt) = pfr_create_ktable(&tbl, gettime(), true, wait) else {
                rs.tables.set(rs.tables.get() - 1);
                return None;
            };
            if ac.is_some() {
                tbl.pfrt_anchor.fill(0);
                let rt = match pfr_lookup_table(&tbl) {
                    Some(rt) => rt,
                    None => {
                        let Some(rt) = pfr_create_ktable(&tbl, 0, true, wait) else {
                            pfr_destroy_ktable(kt, false);
                            rs.tables.set(rs.tables.get() - 1);
                            return None;
                        };
                        pfr_insert_ktable(rt);
                        rt
                    }
                };
                kt.pfrkt_root.set(Some(rt));
            }
            pfr_insert_ktable(kt);
            rs.tables.set(rs.tables.get() - 1);
            kt
        }
    };
    let r = &kt.pfrkt_refcnt()[PFR_REFCNT_RULE];
    let old = r.get();
    r.set(old + 1);
    if old == 0 {
        pfr_setflags_ktable(kt, kt.pfrkt_flags().get() | PFR_TFLAG_REFERENCED);
    }
    Some(kt)
}

/// `pfr_detach_table`: drops a rule's reference on `kt`.
pub fn pfr_detach_table(kt: &'static PfrKtable) {
    let r = &kt.pfrkt_refcnt()[PFR_REFCNT_RULE];
    if r.get() <= 0 {
        crate::dpfprintf!(LOG_NOTICE, "pfr_detach_table: refcount = {}.", r.get());
    } else {
        r.set(r.get() - 1);
        if r.get() == 0 {
            pfr_setflags_ktable(kt, kt.pfrkt_flags().get() & !PFR_TFLAG_REFERENCED);
        }
    }
}

/// `pfr_islinklocal`: whether `addr` is an IPv6 link-local address.
pub fn pfr_islinklocal(af: SaFamily, addr: &PfAddr) -> bool {
    #[cfg(feature = "inet6")]
    if af == AF_INET6 && crate::netinet6::in6::in6_is_addr_linklocal(&addr.v6()) {
        return true;
    }
    #[cfg(not(feature = "inet6"))]
    let _ = (af, addr);
    false
}

/// The kernel `pf_pool`'s table (`PF_ADDR_TABLE`) or dynamic address table
/// (`PF_ADDR_DYNIFTL`).
fn pfr_pool_table(rpool: &'static PfPool) -> Result<Option<&'static PfrKtable>, i32> {
    match rpool.addr.type_.get() {
        PF_ADDR_TABLE => Ok(rpool.addr.tbl()),
        PF_ADDR_DYNIFTL => Ok(rpool.addr.dyn_().and_then(|d| d.pfid_kt.get())),
        _ => Err(-1),
    }
}

/// Sets the pool's position and per-address state after it settled on the entry `ke` at
/// index `idx`, address `addr`.
fn pfr_pool_settle(
    rpool: &'static PfPool,
    kt: &'static PfrKtable,
    ke: &'static PfrKentry,
    idx: i32,
    addr: &PfAddr,
    af: SaFamily,
) {
    let mut counter = rpool.counter.get();
    pf_addrcpy(&mut counter, addr, af);
    rpool.counter.set(counter);
    rpool.tblidx.set(idx);
    kt.pfrkt_match().set(kt.pfrkt_match().get().wrapping_add(1));
    rpool.states.set(0);
    if let Some(c) = ke.pfrke_counters.get() {
        rpool.states.set(c.states.get());
    }
    match ke.pfrke_type.get() {
        t @ (PFRKE_COST | PFRKE_ROUTE) => {
            if t == PFRKE_COST {
                rpool.weight.set(pfr_kentry_cost(ke).weight.get());
            }
            rpool.set_kif(pfr_kentry_route(ke).kif.get());
        }
        _ => rpool.weight.set(1),
    }
}

/// `pfr_pool_get`: the next address of the round-robin pool `rpool` over its table, in
/// family `af`; returns the block the address is in (`*raddr`, `*rmask`) and leaves the
/// address in `rpool.counter`. `Err(1)` when the table has no usable address, `Err(-1)` when
/// the pool is not over a table.
pub fn pfr_pool_get(rpool: &'static PfPool, af: SaFamily) -> Result<(PfAddr, PfAddr), i32> {
    let mut tmp4 = PfsockaddrUnion::default();
    let mut addr = PfAddr::zeroed();
    let mut mask = PfsockaddrUnion::default();
    let mut loop_ = 0;
    let mut use_counter = false;

    match af {
        AF_INET => fillin_sin(&mut tmp4, InAddr { s_addr: 0 }),
        #[cfg(feature = "inet6")]
        AF_INET6 => fillin_sin6(&mut tmp4, In6Addr::default()),
        _ => unhandled_af(i32::from(af)),
    }

    let kt = pfr_pool_table(rpool)?;
    let Some(kt) = kt.and_then(pfr_ktable_select_active) else {
        return Err(-1);
    };

    let mut idx = rpool.tblidx.get();
    if idx < 0 || idx >= kt.pfrkt_cnt().get() {
        idx = 0;
    } else {
        use_counter = true;
    }
    let startidx = idx;

    // _next_block:
    loop {
        if loop_ != 0 && startidx == idx {
            kt.pfrkt_nomatch()
                .set(kt.pfrkt_nomatch().get().wrapping_add(1));
            return Err(1);
        }

        let ke = match pfr_kentry_byidx(kt, idx, af) {
            Some(ke) => ke,
            None => {
                // we don't have this idx, try looping
                let ke = if loop_ != 0 {
                    None
                } else {
                    pfr_kentry_byidx(kt, 0, af)
                };
                let Some(ke) = ke else {
                    kt.pfrkt_nomatch()
                        .set(kt.pfrkt_nomatch().get().wrapping_add(1));
                    return Err(1);
                };
                idx = 0;
                loop_ += 1;
                ke
            }
        };

        // Get current weight for weighted round-robin
        if idx == 0 && use_counter && kt.pfrkt_refcntcost.get() > 0 {
            rpool
                .curweight
                .set(rpool.curweight.get() - i32::from(kt.pfrkt_gcdweight.get()));

            if rpool.curweight.get() < 1 {
                rpool.curweight.set(i32::from(kt.pfrkt_maxweight.get()));
            }
        }

        let mut pmask = PfsockaddrUnion::default();
        pfr_prepare_network(&mut pmask, af, i32::from(ke.pfrke_net.get()));
        PFR_MASK.set(pmask);
        let raddr = sunion2pf(&ke.pfrke_sa.get(), af);
        let rmask = sunion2pf(&PFR_MASK.get(), af);

        let counter = rpool.counter.get();
        if use_counter && !pf_azero(&counter, af) {
            // is supplied address within block?
            if !pf_match_addr(0, &raddr, &rmask, &counter, af) {
                // no, go to next block in table
                idx += 1;
                use_counter = false;
                continue;
            }
            pf_addrcpy(&mut addr, &counter, af);
        } else {
            // use first address of block
            pf_addrcpy(&mut addr, &raddr, af);
        }

        if !kentry_network(ke) {
            // this is a single IP address - no possible nested block
            if rpool.addr.type_.get() == PF_ADDR_DYNIFTL && pfr_islinklocal(af, &addr) {
                idx += 1;
                continue;
            }
            pfr_pool_settle(rpool, kt, ke, idx, &addr, af);
            return Ok((raddr, rmask));
        }
        loop {
            // we don't want to use a nested block
            let ke2 = match af {
                AF_INET => {
                    let mut sin = tmp4.sin();
                    sin.sin_addr = InAddr {
                        s_addr: addr.addr32(0),
                    };
                    tmp4.set_sin(&sin);
                    // SAFETY: as in `pfr_kentry_byaddr`.
                    unsafe { rn_match(tmp4.bytes.as_ptr(), pfr_rnh(&kt.pfrkt_ip4)) }.map(rn2ke)
                }
                #[cfg(feature = "inet6")]
                AF_INET6 => {
                    // tmp6, in the same union.
                    let mut sin6 = tmp4.sin6();
                    sin6.sin6_addr = addr.v6();
                    tmp4.set_sin6(&sin6);
                    // SAFETY: as in `pfr_kentry_byaddr`.
                    unsafe { rn_match(tmp4.bytes.as_ptr(), pfr_rnh(&kt.pfrkt_ip6)) }.map(rn2ke)
                }
                _ => unhandled_af(i32::from(af)),
            };
            if ke2.is_some_and(|ke2| ptr::eq(ke2, ke))
                && !(rpool.addr.type_.get() == PF_ADDR_DYNIFTL && pfr_islinklocal(af, &addr))
            {
                // lookup return the same block - perfect
                pfr_pool_settle(rpool, kt, ke, idx, &addr, af);
                return Ok((raddr, rmask));
            }
            // _next_entry:
            let Some(ke2) = ke2 else {
                panic(format_args!("pfr_pool_get: address outside its block"));
            };
            // we need to increase the counter past the nested block
            pfr_prepare_network(&mut mask, af, i32::from(ke2.pfrke_net.get()));
            let cur = addr;
            pf_poolmask(
                &mut addr,
                &cur,
                &sunion2pf(&mask, af),
                &PFR_FFADDR.get(),
                af,
            );
            pf_addr_inc(&mut addr, af);
            if !pf_match_addr(0, &raddr, &rmask, &addr, af) {
                // ok, we reached the end of our main block
                // go to next block in table
                idx += 1;
                use_counter = false;
                break;
            }
        }
    }
}

/// `pfr_kentry_byidx`: the `idx`th positive entry of family `af` in `kt`.
pub fn pfr_kentry_byidx(
    kt: &'static PfrKtable,
    idx: i32,
    af: SaFamily,
) -> Option<&'static PfrKentry> {
    let mut w = PfrWalktree::new(PfrwOp::PoolGet);
    w.pfrw_free = idx;

    match af {
        AF_INET => {
            let _ = pfr_walk(pfr_rnh(&kt.pfrkt_ip4), &mut w);
            match w.pfrw_1 {
                Pfrw1::Kentry(ke) => ke,
                _ => None,
            }
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            let _ = pfr_walk(pfr_rnh(&kt.pfrkt_ip6), &mut w);
            match w.pfrw_1 {
                Pfrw1::Kentry(ke) => ke,
                _ => None,
            }
        }
        _ => None,
    }
}

/// `pfr_states_increase`: counts one more state on the entry for `addr` (added for load
/// balancing state counter use); its count, -1 without such an entry or memory.
pub fn pfr_states_increase(kt: &'static PfrKtable, addr: &PfAddr, af: SaFamily) -> i32 {
    let Some(ke) = pfr_kentry_byaddr(kt, addr, af, true) else {
        return -1;
    };

    if ke.pfrke_counters.get().is_none() {
        ke.pfrke_counters.set(pf_pool_get::<PfrKcounters>(
            &PFR_KCOUNTERS_PL,
            PR_NOWAIT | PR_ZERO,
        ));
    }
    let Some(c) = ke.pfrke_counters.get() else {
        return -1;
    };

    c.states.set(c.states.get().wrapping_add(1));
    c.states.get() as i32
}

/// `pfr_states_decrease`: counts one state less on the entry for `addr` (added for load
/// balancing state counter use); its count, -1 without such an entry or memory.
pub fn pfr_states_decrease(kt: &'static PfrKtable, addr: &PfAddr, af: SaFamily) -> i32 {
    let Some(ke) = pfr_kentry_byaddr(kt, addr, af, true) else {
        return -1;
    };

    if ke.pfrke_counters.get().is_none() {
        ke.pfrke_counters.set(pf_pool_get::<PfrKcounters>(
            &PFR_KCOUNTERS_PL,
            PR_NOWAIT | PR_ZERO,
        ));
    }
    let Some(c) = ke.pfrke_counters.get() else {
        return -1;
    };

    if c.states.get() > 0 {
        c.states.set(c.states.get() - 1);
    } else {
        crate::dpfprintf!(LOG_DEBUG, "pfr_states_decrease: states-- when states <= 0");
    }

    c.states.get() as i32
}

/// `pfr_dynaddr_update`: sets the first address and mask of each family of the table `kt` in
/// the dynamic address `dyn_`, and counts the addresses.
pub fn pfr_dynaddr_update(kt: &'static PfrKtable, dyn_: &'static PfiDynaddr) {
    let mut w = PfrWalktree::new(PfrwOp::DynaddrUpdate);
    w.pfrw_1 = Pfrw1::Dyn(dyn_);

    dyn_.pfid_acnt4.set(0);
    dyn_.pfid_acnt6.set(0);
    match dyn_.pfid_af.get() {
        AF_UNSPEC => {
            // look up all both addresses IPv4 + IPv6
            let _ = pfr_walk(pfr_rnh(&kt.pfrkt_ip4), &mut w);
            let _ = pfr_walk(pfr_rnh(&kt.pfrkt_ip6), &mut w);
        }
        AF_INET => {
            let _ = pfr_walk(pfr_rnh(&kt.pfrkt_ip4), &mut w);
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            let _ = pfr_walk(pfr_rnh(&kt.pfrkt_ip6), &mut w);
        }
        af => unhandled_af(i32::from(af)),
    }
}

/// `pfr_ktable_winfo_update`: with cost entries, the table's weight gcd (needed for
/// round-robin) and maximum account for the entry `p`.
pub fn pfr_ktable_winfo_update(kt: &'static PfrKtable, p: &'static PfrKentry) {
    // If cost flag is set, gcdweight is needed for round-robin.
    if kt.pfrkt_refcntcost.get() > 0 {
        let weight: u16 = if p.pfrke_type.get() == PFRKE_COST {
            pfr_kentry_cost(p).weight.get()
        } else {
            1
        };

        if kt.pfrkt_gcdweight.get() == 0 {
            kt.pfrkt_gcdweight.set(weight);
        }

        kt.pfrkt_gcdweight
            .set(pfr_gcd(i32::from(weight), i32::from(kt.pfrkt_gcdweight.get())) as u16);

        if kt.pfrkt_maxweight.get() < weight {
            kt.pfrkt_maxweight.set(weight);
        }
    }
}

/// `pfr_ktable_select_active`: `kt` if active, else its root table if that is active.
pub fn pfr_ktable_select_active(kt: &'static PfrKtable) -> Option<&'static PfrKtable> {
    let mut kt = kt;
    if kt.pfrkt_flags().get() & PFR_TFLAG_ACTIVE == 0
        && let Some(root) = kt.pfrkt_root.get()
    {
        kt = root;
    }
    if kt.pfrkt_flags().get() & PFR_TFLAG_ACTIVE == 0 {
        return None;
    }

    Some(kt)
}

#[cfg(test)]
mod tests;
