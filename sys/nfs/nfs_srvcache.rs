/*	$OpenBSD: nfs_srvcache.c,v 1.32 2024/09/18 05:21:19 jsg Exp $	*/
/*	$NetBSD: nfs_srvcache.c,v 1.12 1996/02/18 11:53:49 fvdl Exp $	*/
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
 *	@(#)nfs_srvcache.c	8.3 (Berkeley) 3/30/95
 */
/* </LICENSES> */

/* <CODE> */
//! `nfs/nfs_srvcache.c`: the NFS server's recent request cache (the duplicate request
//! cache): requests of datagram transports are remembered by transaction id, procedure and
//! client address, so that a retransmission of a request still in progress is dropped, one
//! of a non-idempotent request that has completed gets the saved reply, and one of an
//! idempotent request is simply done again.
//!
//! Upstream: sys/nfs/nfs_srvcache.c @ 3ce1f3f79392
//!
//! Reference: Chet Juszczak, "Improving the Performance and Correctness of an NFS Server",
//! in Proc. Winter 1989 USENIX Conference, pages 53-63. San Diego, February 1989.
//!
//! The file is compiled only with `option NFSSERVER`: the module is behind the `nfsserver`
//! feature.
//!
//! ## Deviations
//! - The globals (`numnfsrvcache`, `desirednfsrvcache`, the hash table, its key and mask, the
//!   LRU list) are statics with upper-case names: atomics for the counts, `StaticCell`s for
//!   the table, the mask and the key (written once by `nfsrv_initcache`, before the first
//!   `nfsd` runs), and the LRU list in a `Sync` wrapper; the list and the entries are changed
//!   under the kernel lock, as in C.
//! - `nfsrv_getcache` returns the reply through `repp: &mut Option<&'static Mbuf>`, as the C's
//!   `struct mbuf **`. When `m_copym` or `malloc` cannot get memory (the C's `M_WAIT` cannot
//!   fail; here the pools cannot sleep): a reply that cannot be copied drops the request
//!   (`RC_DROPIT`; the client retransmits), a cache entry that cannot be allocated lets the
//!   request through uncached (`RC_DOIT`), and `nfsrv_updatecache` leaves the entry without
//!   a saved reply (a retransmission is then done again, as for an idempotent request); the C
//!   would store a null reply and fault on the next hit.
//! - `nfsrv_updatecache` takes `repvalid` as a `bool` and the reply as an `Option` (the C
//!   passes a null `mreq` with `repvalid` 0 on the error path); `repvalid` with no reply is a
//!   panic.
//! - `netaddr_match` and `NETFAMILY` (`nfs_srvsubs.rs`, here) compare in `AF_INET` only, as
//!   the C does: an entry of any other family (`RC_NAM`) never matches a retransmission.

use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI64, Ordering};

use libkern::StaticCell;

use crate::crypto::siphash::{SipHash24, SiphashKey};
use crate::dev::rnd::arc4random_buf;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_subr::hashinit;
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{m_copym, m_free, m_freem};
use crate::nfs::nfs::{ND_NFSV3, NfsrvDescript, NfssvcSock};
use crate::nfs::nfs_socket::nfs_rephead;
use crate::nfs::nfs_srvsubs::{netaddr_match, sockaddr_in_of};
use crate::nfs::nfs_subs::{NFSSTATS, NFSV2_PROCID};
use crate::nfs::nfsproto::NFS_NPROCS;
use crate::nfs::nfsrvcache::{
    NFSRVCACHESIZ, NfsrvCache, RC_DOIT, RC_DONE, RC_DROPIT, RC_INETADDR, RC_INPROG, RC_LOCKED,
    RC_NAM, RC_REPLY, RC_REPMBUF, RC_REPSTATUS, RC_UNUSED, RC_WANTED, RcHash, RcLru,
};
use crate::sys::malloc::{M_NFSD, M_WAITOK, M_ZERO};
use crate::sys::mbuf::{M_COPYALL, M_WAIT, Mbuf};
use crate::sys::param::PZERO;
use crate::sys::queue::{ListHead, TailqHead};
use crate::sys::socket::{AF_INET, AF_UNSPEC};
use crate::sys::systm::INFSLP;
use crate::sys::types::SaFamily;

/// The hash chains, with the claim that makes them shareable.
#[derive(Clone, Copy)]
struct Nfsrvhashtbl(&'static [ListHead<RcHash>]);

// SAFETY: the chains are changed under the kernel lock, as in C.
unsafe impl Sync for Nfsrvhashtbl {}
// SAFETY: as above.
unsafe impl Send for Nfsrvhashtbl {}

/// `TAILQ_HEAD(nfsrvlru, nfsrvcache)`: the cache's entries, least recently used first.
struct Nfsrvlru(TailqHead<RcLru>);

// SAFETY: the list is changed under the kernel lock, as in C.
unsafe impl Sync for Nfsrvlru {}

/// `numnfsrvcache`: the number of entries in the cache.
pub static NUMNFSRVCACHE: AtomicI64 = AtomicI64::new(0);
/// `desirednfsrvcache`: the number of entries the cache grows to before it recycles.
pub static DESIREDNFSRVCACHE: AtomicI64 = AtomicI64::new(NFSRVCACHESIZ as i64);

/// `LIST_HEAD(nfsrvhash, nfsrvcache) *nfsrvhashtbl`.
static NFSRVHASHTBL: StaticCell<Nfsrvhashtbl> = StaticCell::new(Nfsrvhashtbl(&[]));
/// `nfsrvhash`: size of the hash table - 1.
static NFSRVHASH: StaticCell<u64> = StaticCell::new(0);
/// `nfsrvhashkey`.
static NFSRVHASHKEY: StaticCell<SiphashKey> = StaticCell::new(SiphashKey { k0: 0, k1: 0 });
/// `nfsrvlruhead`.
static NFSRVLRUHEAD: Nfsrvlru = Nfsrvlru(TailqHead::new());

/// `nonidempotent[NFS_NPROCS]`: which NFS RPCs are nonidempotent.
const NONIDEMPOTENT: [bool; NFS_NPROCS] = [
    false, false, true, false, false, false, false, true, true, true, true, true, true, true, true,
    true, false, false, false, false, false, false, false,
];

/// `nfsv2_repstat[NFS_NPROCS]`: true iff the RPC reply is an NFS status ONLY! Indexed by the
/// version 2 procedure number.
const NFSV2_REPSTAT: [bool; NFS_NPROCS] = [
    false, false, false, false, false, false, false, false, false, false, true, true, true, true,
    false, true, false, false, false, false, false, false, false,
];

/// `NETFAMILY(rp)`: the family of the address an entry holds.
fn netfamily(rp: &NfsrvCache) -> SaFamily {
    if rp.rc_flag.get() & RC_INETADDR != 0 {
        AF_INET
    } else {
        AF_UNSPEC
    }
}

/// `NFSRCHASH(xid)`: the chain of a transaction id.
fn nfsrchash(xid: u32) -> &'static ListHead<RcHash> {
    // SAFETY: the three cells are written only by `nfsrv_initcache`, before the server runs,
    // and read-only afterwards.
    let (tbl, mask, key) = unsafe { (NFSRVHASHTBL.get().0, *NFSRVHASH.get(), NFSRVHASHKEY.get()) };
    if tbl.is_empty() {
        panic(format_args!("nfsrchash: no table"));
    }
    &tbl[(SipHash24(key, &xid.to_ne_bytes()) & mask) as usize]
}

/// Wakes whoever waits for the entry `rp` (`wakeup(rp)`).
fn wakeup_entry(rp: &NfsrvCache) {
    wakeup(ptr::from_ref(rp));
}

/// Unlocks the entry and wakes the waiters, as the C does at the end of each user.
fn unlock_entry(rp: &NfsrvCache) {
    rp.rc_flag.set(rp.rc_flag.get() & !RC_LOCKED);
    if rp.rc_flag.get() & RC_WANTED != 0 {
        rp.rc_flag.set(rp.rc_flag.get() & !RC_WANTED);
        wakeup_entry(rp);
    }
}

/// Sleeps until the locked entry `rp` is unlocked (`rc_flag |= RC_WANTED; tsleep(rp)`).
fn sleep_on_entry(rp: &NfsrvCache) {
    rp.rc_flag.set(rp.rc_flag.get() | RC_WANTED);
    let _ = tsleep_nsec(ptr::from_ref(rp), PZERO - 1, "nfsrc", INFSLP);
}

/// `nfsrv_cleanentry(rp)`: frees what the entry holds: the saved reply and the copy of the
/// client's address.
pub fn nfsrv_cleanentry(rp: &NfsrvCache) {
    if rp.rc_flag.get() & RC_REPMBUF != 0 {
        m_freem(rp.rc_reply.take());
    }

    if rp.rc_flag.get() & RC_NAM != 0 {
        m_free(rp.rc_nam());
        rp.set_rc_nam(None);
    }

    rp.rc_flag
        .set(rp.rc_flag.get() & !(RC_REPSTATUS | RC_REPMBUF));
}

/// `nfsrv_initcache()`: initialize the server request cache list.
pub fn nfsrv_initcache() {
    let desired = DESIREDNFSRVCACHE.load(Ordering::Relaxed) as i32;
    let Some(tbl) = hashinit::<RcHash>(desired, M_NFSD, M_WAITOK) else {
        panic(format_args!("nfsrv_initcache: no memory"));
    };
    let mut key = [0u8; 16];
    arc4random_buf(&mut key);
    let mut k0 = [0u8; 8];
    let mut k1 = [0u8; 8];
    k0.copy_from_slice(&key[..8]);
    k1.copy_from_slice(&key[8..]);
    // SAFETY: called once, from `nfs_init`, before any `nfsd` runs, so nothing reads the cells
    // meanwhile (the module's deviations).
    unsafe {
        NFSRVHASHTBL.write(Nfsrvhashtbl(tbl));
        NFSRVHASH.write(tbl.len() as u64 - 1);
        NFSRVHASHKEY.write(SiphashKey {
            k0: u64::from_ne_bytes(k0),
            k1: u64::from_ne_bytes(k1),
        });
    }
    NFSRVLRUHEAD.0.init();
}

/// `nfsrv_getcache(nd, slp, repp)`: looks for the request in the cache. If found, returns the
/// action and optionally the reply (in `*repp`); otherwise inserts it in the cache.
///
/// The rules are as follows:
/// - if in progress, return `RC_DROPIT`;
/// - if completed within DELAY of the current time, return `RC_DROPIT`;
/// - if completed a longer time ago, return `RC_REPLY` if the reply was cached or `RC_DOIT`.
///
/// Update/add the new request at the end of the LRU list. Requests that did not come on a
/// datagram transport (`nd_nam2` unset) are not cached: `RC_DOIT`.
pub fn nfsrv_getcache(
    nd: &NfsrvDescript,
    slp: &NfssvcSock,
    repp: &mut Option<&'static Mbuf>,
) -> i32 {
    // Don't cache recent requests for reliable transport protocols. (Maybe we should for the
    // case of a reconnect, but..)
    if nd.nd_nam2.is_none() {
        return RC_DOIT;
    }
    let Some(nam) = nd.nd_nam else {
        panic(format_args!("nfsrv_getcache: no client address"));
    };

    if let Some(rp) = nfsrv_lookupcache(nd) {
        // If not at end of LRU chain, move it there
        if TailqHead::<RcLru>::next(rp).is_some() {
            // SAFETY: `rp` is on the LRU list (`nfsrv_lookupcache` found it through the hash,
            // and entries are on both or neither), and is put back at once.
            unsafe {
                NFSRVLRUHEAD.0.remove(rp);
                NFSRVLRUHEAD.0.insert_tail(rp);
            }
        }
        if rp.rc_state.get() == RC_UNUSED {
            panic(format_args!("nfsrv cache"));
        }
        let ret = if rp.rc_state.get() == RC_INPROG {
            NFSSTATS.srvcache_inproghits.fetch_add(1, Ordering::Relaxed);
            RC_DROPIT
        } else if rp.rc_flag.get() & RC_REPSTATUS != 0 {
            NFSSTATS
                .srvcache_nonidemdonehits
                .fetch_add(1, Ordering::Relaxed);
            match nfs_rephead(0, nd, Some(slp), rp.rc_status.get()) {
                Ok((reply, _)) => {
                    *repp = Some(reply);
                    RC_REPLY
                }
                Err(_) => RC_DROPIT,
            }
        } else if rp.rc_flag.get() & RC_REPMBUF != 0 {
            NFSSTATS
                .srvcache_nonidemdonehits
                .fetch_add(1, Ordering::Relaxed);
            match rp
                .rc_reply
                .get()
                .and_then(|r| m_copym(r, 0, M_COPYALL, M_WAIT))
            {
                Some(copy) => {
                    *repp = Some(copy);
                    RC_REPLY
                }
                None => RC_DROPIT,
            }
        } else {
            NFSSTATS
                .srvcache_idemdonehits
                .fetch_add(1, Ordering::Relaxed);
            rp.rc_state.set(RC_INPROG);
            RC_DOIT
        };
        unlock_entry(rp);
        return ret;
    }

    NFSSTATS.srvcache_misses.fetch_add(1, Ordering::Relaxed);
    let rp: &'static NfsrvCache;
    if NUMNFSRVCACHE.load(Ordering::Relaxed) < DESIREDNFSRVCACHE.load(Ordering::Relaxed) {
        let Some(p) = malloc(size_of::<NfsrvCache>(), M_NFSD, M_WAITOK | M_ZERO) else {
            // The C's M_WAITOK cannot fail (the module's deviations): no entry, no caching.
            return RC_DOIT;
        };
        let p = p.cast::<NfsrvCache>();
        // SAFETY: a fresh, zeroed `malloc` item of the size of an `NfsrvCache`, suitably
        // aligned (malloc's blocks are), initialised here and kept until `nfsrv_cleancache`
        // or the process of the kernel ends.
        rp = unsafe {
            p.as_ptr().write(NfsrvCache::new());
            &*p.as_ptr()
        };
        NUMNFSRVCACHE.fetch_add(1, Ordering::Relaxed);
        rp.rc_flag.set(RC_LOCKED);
    } else {
        let Some(mut cur) = NFSRVLRUHEAD.0.first() else {
            panic(format_args!("nfsrv_getcache: empty LRU list"));
        };
        while cur.rc_flag.get() & RC_LOCKED != 0 {
            sleep_on_entry(cur);
            let Some(f) = NFSRVLRUHEAD.0.first() else {
                panic(format_args!("nfsrv_getcache: empty LRU list"));
            };
            cur = f;
        }
        rp = cur;
        rp.rc_flag.set(rp.rc_flag.get() | RC_LOCKED);
        // SAFETY: `rp` is in the hash (every entry on the LRU list is) and on the LRU list.
        unsafe {
            ListHead::<RcHash>::remove(rp);
            NFSRVLRUHEAD.0.remove(rp);
        }
        nfsrv_cleanentry(rp);
        rp.rc_flag.set(rp.rc_flag.get() & (RC_LOCKED | RC_WANTED));
    }
    // SAFETY: `rp` is on no list (new, or just removed from both), and lives until
    // `nfsrv_cleancache`.
    unsafe { NFSRVLRUHEAD.0.insert_tail(rp) };
    rp.rc_state.set(RC_INPROG);
    rp.rc_xid.set(nd.nd_retxid);
    match sockaddr_in_of(nam) {
        Some(saddr) if saddr.sin_family == AF_INET => {
            rp.rc_flag.set(rp.rc_flag.get() | RC_INETADDR);
            rp.set_rc_inetaddr(saddr.sin_addr.s_addr);
        }
        _ => {
            rp.rc_flag.set(rp.rc_flag.get() | RC_NAM);
            rp.set_rc_nam(m_copym(nam, 0, M_COPYALL, M_WAIT));
        }
    }
    rp.rc_proc.set(nd.nd_procnum as u16);
    // SAFETY: `rp` is on no hash chain (see above).
    unsafe { nfsrchash(nd.nd_retxid).insert_head(rp) };
    unlock_entry(rp);
    RC_DOIT
}

/// `nfsrv_updatecache(nd, repvalid, repmbuf)`: updates a request cache entry after the RPC
/// has been done: it is `RC_DONE`, and with a valid reply of a non-idempotent procedure the
/// reply (or, for a version 2 procedure whose reply is only an NFS status, the status) is
/// saved.
pub fn nfsrv_updatecache(nd: &NfsrvDescript, repvalid: bool, repmbuf: Option<&Mbuf>) {
    if nd.nd_nam2.is_none() {
        return;
    }

    if let Some(rp) = nfsrv_lookupcache(nd) {
        nfsrv_cleanentry(rp);
        rp.rc_state.set(RC_DONE);
        // If we have a valid reply update status and save the reply for non-idempotent rpc's.
        if repvalid && NONIDEMPOTENT[nd.nd_procnum] {
            if nd.nd_flag & ND_NFSV3 == 0 && NFSV2_REPSTAT[NFSV2_PROCID[nd.nd_procnum]] {
                rp.rc_status.set(nd.nd_repstat);
                rp.rc_flag.set(rp.rc_flag.get() | RC_REPSTATUS);
            } else {
                let Some(repmbuf) = repmbuf else {
                    panic(format_args!(
                        "nfsrv_updatecache: valid reply without a reply"
                    ));
                };
                if let Some(copy) = m_copym(repmbuf, 0, M_COPYALL, M_WAIT) {
                    rp.rc_reply.set(Some(copy));
                    rp.rc_flag.set(rp.rc_flag.get() | RC_REPMBUF);
                }
            }
        }
        unlock_entry(rp);
    }
}

/// `nfsrv_cleancache()`: cleans out the cache. Called when the last nfsd terminates.
pub fn nfsrv_cleancache() {
    let mut cur = NFSRVLRUHEAD.0.first();
    while let Some(rp) = cur {
        cur = TailqHead::<RcLru>::next(rp);
        // SAFETY: `rp` is on the hash chain and the LRU list, and nothing uses it afterwards.
        unsafe {
            ListHead::<RcHash>::remove(rp);
            NFSRVLRUHEAD.0.remove(rp);
        }
        nfsrv_cleanentry(rp);
        free(
            NonNull::from(rp).cast::<u8>(),
            M_NFSD,
            size_of::<NfsrvCache>(),
        );
    }
    NUMNFSRVCACHE.store(0, Ordering::Relaxed);
}

/// `nfsrv_lookupcache(nd)`: finds the entry of the request, locked (waiting while somebody
/// else holds it), or `None`.
pub fn nfsrv_lookupcache(nd: &NfsrvDescript) -> Option<&'static NfsrvCache> {
    let nam = nd.nd_nam?;
    let hash = nfsrchash(nd.nd_retxid);
    'search: loop {
        for rp in hash.iter() {
            if nd.nd_retxid == rp.rc_xid.get()
                && nd.nd_procnum == rp.rc_proc.get() as usize
                && netaddr_match(netfamily(rp), &rp.rc_haddr.get(), nam)
            {
                if rp.rc_flag.get() & RC_LOCKED != 0 {
                    sleep_on_entry(rp);
                    continue 'search;
                }
                rp.rc_flag.set(rp.rc_flag.get() | RC_LOCKED);
                return Some(rp);
            }
        }
        return None;
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `nfs_srvcache.c`: the cache's three answers (drop, reply, do it), the LRU
    // recycling, the hash lookup by transaction id, procedure and client address, and the
    // clean-up.

    use std::vec::Vec;

    use super::*;
    use crate::kern::uipc_mbuf::m_get;
    use crate::kern::uipc_mbuf::tests::setup as mbuf_setup;
    use crate::nfs::nfs_subs::tests::{bytes, chain};
    use crate::nfs::nfsproto::{NFSPROC_GETATTR, NFSPROC_REMOVE, NFSPROC_SETATTR, NFSPROC_WRITE};
    use crate::sys::mbuf::{M_DONTWAIT, MT_SONAME, mtod};
    use std::sync::MutexGuard;

    /// Memory, mbufs, and an empty cache of at most `desired` entries.
    fn fresh(desired: i64) -> MutexGuard<'static, ()> {
        let g = mbuf_setup();
        DESIREDNFSRVCACHE.store(desired, Ordering::Relaxed);
        NUMNFSRVCACHE.store(0, Ordering::Relaxed);
        nfsrv_initcache();
        g
    }

    /// An `MT_SONAME` mbuf holding the `AF_INET` address `a`, port `port`.
    fn nam(a: [u8; 4], port: u16) -> &'static Mbuf {
        let m = m_get(M_DONTWAIT, MT_SONAME).expect("an mbuf");
        let mut sin = [0u8; 16];
        sin[0] = 16;
        sin[1] = AF_INET;
        sin[2..4].copy_from_slice(&port.to_be_bytes());
        sin[4..8].copy_from_slice(&a);
        // SAFETY: an mbuf's data area holds 16 bytes; `mtod` points at it.
        unsafe { ptr::copy_nonoverlapping(sin.as_ptr(), mtod::<u8>(m), 16) };
        m.m_len().set(16);
        m
    }

    /// A datagram request: transaction id, procedure, client.
    fn req(xid: u32, procnum: usize, client: [u8; 4], v3: bool) -> NfsrvDescript {
        let mut nd = NfsrvDescript::new();
        nd.nd_retxid = xid;
        nd.nd_procnum = procnum;
        nd.nd_nam = Some(nam(client, 700));
        nd.nd_nam2 = Some(m_get(M_DONTWAIT, MT_SONAME).expect("an mbuf"));
        if v3 {
            nd.nd_flag |= ND_NFSV3;
        }
        nd
    }

    /// The number of entries on the LRU list.
    fn lru_len() -> usize {
        NFSRVLRUHEAD.0.iter().count()
    }

    /// The xids on the LRU list, least recently used first.
    fn lru_xids() -> Vec<u32> {
        NFSRVLRUHEAD.0.iter().map(|rp| rp.rc_xid.get()).collect()
    }

    #[test]
    fn the_tables_match_the_c_arrays() {
        // nonidempotent: setattr, write, create ... link.
        let non: Vec<usize> = (0..NFS_NPROCS).filter(|&i| NONIDEMPOTENT[i]).collect();
        assert_eq!(non, [2, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
        // nfsv2_repstat: remove, rename, link, mkdir? (v2 numbers 10, 11, 12, 13, 15)
        let st: Vec<usize> = (0..NFS_NPROCS).filter(|&i| NFSV2_REPSTAT[i]).collect();
        assert_eq!(st, [10, 11, 12, 13, 15]);
    }

    #[test]
    fn a_new_request_is_cached_and_a_retransmission_in_progress_is_dropped() {
        let _g = fresh(8);
        let slp = NfssvcSock::new();
        let nd = req(0x1234, NFSPROC_GETATTR, [10, 0, 2, 9], true);
        let mut rep = None;

        let m0 = NFSSTATS.srvcache_misses.load(Ordering::Relaxed);
        assert_eq!(nfsrv_getcache(&nd, &slp, &mut rep), RC_DOIT);
        assert_eq!(NFSSTATS.srvcache_misses.load(Ordering::Relaxed), m0 + 1);
        assert_eq!(NUMNFSRVCACHE.load(Ordering::Relaxed), 1);
        assert_eq!(lru_len(), 1);
        let rp = NFSRVLRUHEAD.0.first().expect("an entry");
        assert_eq!(rp.rc_state.get(), RC_INPROG);
        assert_eq!(rp.rc_flag.get(), RC_INETADDR, "unlocked again");
        assert_eq!(rp.rc_inetaddr(), u32::from_ne_bytes([10, 0, 2, 9]));
        assert_eq!(rp.rc_proc.get() as usize, NFSPROC_GETATTR);
        assert!(rep.is_none());

        // The client retransmits while the first is being served.
        let h0 = NFSSTATS.srvcache_inproghits.load(Ordering::Relaxed);
        assert_eq!(nfsrv_getcache(&nd, &slp, &mut rep), RC_DROPIT);
        assert_eq!(NFSSTATS.srvcache_inproghits.load(Ordering::Relaxed), h0 + 1);
        assert_eq!(NUMNFSRVCACHE.load(Ordering::Relaxed), 1, "no second entry");
    }

    #[test]
    fn an_idempotent_request_is_done_again_after_it_completed() {
        let _g = fresh(8);
        let slp = NfssvcSock::new();
        let nd = req(1, NFSPROC_GETATTR, [10, 0, 2, 9], true);
        let mut rep = None;
        assert_eq!(nfsrv_getcache(&nd, &slp, &mut rep), RC_DOIT);

        let reply = chain(&[&[1, 2, 3, 4]]);
        nfsrv_updatecache(&nd, true, Some(reply));
        let rp = NFSRVLRUHEAD.0.first().expect("an entry");
        assert_eq!(rp.rc_state.get(), RC_DONE);
        assert_eq!(
            rp.rc_flag.get() & (RC_REPMBUF | RC_REPSTATUS),
            0,
            "idempotent: no reply saved"
        );

        let i0 = NFSSTATS.srvcache_idemdonehits.load(Ordering::Relaxed);
        assert_eq!(nfsrv_getcache(&nd, &slp, &mut rep), RC_DOIT);
        assert_eq!(
            NFSSTATS.srvcache_idemdonehits.load(Ordering::Relaxed),
            i0 + 1
        );
        assert_eq!(rp.rc_state.get(), RC_INPROG, "being served once more");
        assert_eq!(NUMNFSRVCACHE.load(Ordering::Relaxed), 1);
        m_freem(reply);
    }

    #[test]
    fn a_non_idempotent_reply_is_saved_and_copied_for_a_retransmission() {
        let _g = fresh(8);
        let slp = NfssvcSock::new();
        // A version 3 write: the reply mbuf is saved (a copy) whatever the version.
        let nd = req(7, NFSPROC_WRITE, [10, 0, 2, 9], true);
        let mut rep = None;
        assert_eq!(nfsrv_getcache(&nd, &slp, &mut rep), RC_DOIT);

        let reply = chain(&[&[9, 8, 7], &[6, 5]]);
        nfsrv_updatecache(&nd, true, Some(reply));
        m_freem(reply); // the caller's own reply goes out and is freed; the cache has a copy
        let rp = NFSRVLRUHEAD.0.first().expect("an entry");
        assert_eq!(rp.rc_flag.get() & (RC_REPMBUF | RC_REPSTATUS), RC_REPMBUF);

        let n0 = NFSSTATS.srvcache_nonidemdonehits.load(Ordering::Relaxed);
        assert_eq!(nfsrv_getcache(&nd, &slp, &mut rep), RC_REPLY);
        assert_eq!(
            NFSSTATS.srvcache_nonidemdonehits.load(Ordering::Relaxed),
            n0 + 1
        );
        let copy = rep.take().expect("the reply");
        assert_eq!(bytes(copy), [9, 8, 7, 6, 5]);
        assert!(
            !ptr::eq(copy, rp.rc_reply.get().expect("the saved reply")),
            "a copy"
        );
        // Still saved for the next retransmission, and not in progress.
        assert_eq!(rp.rc_state.get(), RC_DONE);
        assert_eq!(nfsrv_getcache(&nd, &slp, &mut rep), RC_REPLY);
        m_freem(copy);
        m_freem(rep.take());
    }

    #[test]
    fn a_version_2_status_only_reply_saves_the_status() {
        let _g = fresh(8);
        let slp = NfssvcSock::new();
        // remove is NFSV2PROC_REMOVE (10), a status-only reply in version 2.
        let mut nd = req(9, NFSPROC_REMOVE, [10, 0, 2, 9], false);
        let mut rep = None;
        assert_eq!(nfsrv_getcache(&nd, &slp, &mut rep), RC_DOIT);
        nd.nd_repstat = 13;
        nfsrv_updatecache(&nd, true, None);
        let rp = NFSRVLRUHEAD.0.first().expect("an entry");
        assert_eq!(rp.rc_flag.get() & (RC_REPMBUF | RC_REPSTATUS), RC_REPSTATUS);
        assert_eq!(rp.rc_status.get(), 13);

        // The same procedure in version 3 saves the reply mbuf instead.
        let nd3 = req(10, NFSPROC_REMOVE, [10, 0, 2, 9], true);
        assert_eq!(nfsrv_getcache(&nd3, &slp, &mut rep), RC_DOIT);
        let reply = chain(&[&[1]]);
        nfsrv_updatecache(&nd3, true, Some(reply));
        m_freem(reply);
        let rp3 = nfsrv_lookupcache(&nd3).expect("the entry");
        assert_eq!(rp3.rc_flag.get() & (RC_REPMBUF | RC_REPSTATUS), RC_REPMBUF);
        unlock_entry(rp3);

        // A reply that is not valid (the procedure failed) is not saved either way.
        let nd4 = req(11, NFSPROC_SETATTR, [10, 0, 2, 9], true);
        assert_eq!(nfsrv_getcache(&nd4, &slp, &mut rep), RC_DOIT);
        nfsrv_updatecache(&nd4, false, None);
        let rp4 = nfsrv_lookupcache(&nd4).expect("the entry");
        assert_eq!(rp4.rc_state.get(), RC_DONE);
        assert_eq!(rp4.rc_flag.get() & (RC_REPMBUF | RC_REPSTATUS), 0);
        unlock_entry(rp4);
    }

    #[test]
    fn requests_differ_by_xid_procedure_and_client() {
        let _g = fresh(16);
        let slp = NfssvcSock::new();
        let mut rep = None;
        let base = req(5, NFSPROC_GETATTR, [10, 0, 2, 9], true);
        assert_eq!(nfsrv_getcache(&base, &slp, &mut rep), RC_DOIT);
        for other in [
            req(6, NFSPROC_GETATTR, [10, 0, 2, 9], true),
            req(5, NFSPROC_SETATTR, [10, 0, 2, 9], true),
            req(5, NFSPROC_GETATTR, [10, 0, 2, 10], true),
        ] {
            assert_eq!(nfsrv_getcache(&other, &slp, &mut rep), RC_DOIT);
        }
        assert_eq!(NUMNFSRVCACHE.load(Ordering::Relaxed), 4);
        assert_eq!(lru_len(), 4);
        // The port does not take part: the same host from another port is a retransmission.
        let mut same = req(5, NFSPROC_GETATTR, [10, 0, 2, 9], true);
        same.nd_nam = Some(nam([10, 0, 2, 9], 999));
        assert_eq!(nfsrv_getcache(&same, &slp, &mut rep), RC_DROPIT);
    }

    #[test]
    fn requests_on_a_reliable_transport_are_not_cached() {
        let _g = fresh(8);
        let slp = NfssvcSock::new();
        let mut nd = req(1, NFSPROC_GETATTR, [10, 0, 2, 9], true);
        nd.nd_nam2 = None;
        let mut rep = None;
        assert_eq!(nfsrv_getcache(&nd, &slp, &mut rep), RC_DOIT);
        assert_eq!(nfsrv_getcache(&nd, &slp, &mut rep), RC_DOIT);
        nfsrv_updatecache(&nd, true, None);
        assert_eq!(lru_len(), 0);
        assert_eq!(NUMNFSRVCACHE.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_full_cache_recycles_the_least_recently_used_entry() {
        let _g = fresh(3);
        let slp = NfssvcSock::new();
        let mut rep = None;
        let mk = |xid| req(xid, NFSPROC_WRITE, [10, 0, 2, 9], true);
        let (a, b, c, d) = (mk(1), mk(2), mk(3), mk(4));
        for nd in [&a, &b, &c] {
            assert_eq!(nfsrv_getcache(nd, &slp, &mut rep), RC_DOIT);
        }
        let reply = chain(&[&[1, 2]]);
        nfsrv_updatecache(&a, true, Some(reply));
        m_freem(reply);
        assert_eq!(lru_xids(), [1, 2, 3]);

        // A hit moves the entry to the end of the LRU list.
        assert_eq!(nfsrv_getcache(&a, &slp, &mut rep), RC_REPLY);
        m_freem(rep.take());
        assert_eq!(lru_xids(), [2, 3, 1]);

        // The fourth request recycles xid 2 (the least recently used): same count, new key.
        assert_eq!(nfsrv_getcache(&d, &slp, &mut rep), RC_DOIT);
        assert_eq!(NUMNFSRVCACHE.load(Ordering::Relaxed), 3);
        assert_eq!(lru_xids(), [3, 1, 4]);
        // The recycled entry no longer answers for xid 2, and the next miss takes xid 3's.
        assert!(nfsrv_lookupcache(&b).is_none());
        assert_eq!(nfsrv_getcache(&b, &slp, &mut rep), RC_DOIT);
        assert_eq!(lru_xids(), [1, 4, 2]);
        assert!(nfsrv_lookupcache(&c).is_none());
        // Xid 1's entry kept its saved reply meanwhile.
        let one = nfsrv_lookupcache(&a).expect("xid 1 is still there");
        assert!(one.rc_flag.get() & RC_REPMBUF != 0);
        unlock_entry(one);
    }

    #[test]
    fn recycling_an_entry_with_a_reply_frees_it() {
        let _g = fresh(1);
        let slp = NfssvcSock::new();
        let mut rep = None;
        let a = req(1, NFSPROC_WRITE, [10, 0, 2, 9], true);
        assert_eq!(nfsrv_getcache(&a, &slp, &mut rep), RC_DOIT);
        let reply = chain(&[&[1, 2]]);
        nfsrv_updatecache(&a, true, Some(reply));
        m_freem(reply);
        let rp = NFSRVLRUHEAD.0.first().expect("an entry");
        assert!(rp.rc_reply.get().is_some());
        let b = req(2, NFSPROC_WRITE, [10, 0, 2, 9], true);
        assert_eq!(nfsrv_getcache(&b, &slp, &mut rep), RC_DOIT);
        assert!(
            ptr::eq(rp, NFSRVLRUHEAD.0.first().expect("the entry")),
            "the same entry"
        );
        assert_eq!(rp.rc_flag.get() & (RC_REPMBUF | RC_REPSTATUS), 0);
        assert_eq!(rp.rc_xid.get(), 2);
    }

    #[test]
    fn an_address_that_is_not_inet_is_kept_as_a_copy() {
        let _g = fresh(4);
        let slp = NfssvcSock::new();
        let mut rep = None;
        let mut nd = req(1, NFSPROC_GETATTR, [10, 0, 2, 9], true);
        let m = nam([1, 2, 3, 4], 5);
        // SAFETY: byte 1 of the address mbuf is `sin_family`.
        unsafe { mtod::<u8>(m).add(1).write(24) }; // AF_INET6
        nd.nd_nam = Some(m);
        assert_eq!(nfsrv_getcache(&nd, &slp, &mut rep), RC_DOIT);
        let rp = NFSRVLRUHEAD.0.first().expect("an entry");
        assert_eq!(rp.rc_flag.get() & (RC_INETADDR | RC_NAM), RC_NAM);
        let copy = rp.rc_nam().expect("a copy of the address");
        assert!(!ptr::eq(copy, m));
        assert_eq!(bytes(copy), bytes(m));
        // As in C, an entry of another family never matches: the retransmission is a new entry.
        assert_eq!(nfsrv_getcache(&nd, &slp, &mut rep), RC_DOIT);
        assert_eq!(lru_len(), 2);
        nfsrv_cleancache();
    }

    #[test]
    fn cleancache_empties_everything() {
        let _g = fresh(8);
        let slp = NfssvcSock::new();
        let mut rep = None;
        for xid in 1..=5 {
            let nd = req(xid, NFSPROC_WRITE, [10, 0, 2, 9], true);
            assert_eq!(nfsrv_getcache(&nd, &slp, &mut rep), RC_DOIT);
            let reply = chain(&[&[xid as u8]]);
            nfsrv_updatecache(&nd, true, Some(reply));
            m_freem(reply);
        }
        assert_eq!(lru_len(), 5);
        nfsrv_cleancache();
        assert_eq!(lru_len(), 0);
        assert_eq!(NUMNFSRVCACHE.load(Ordering::Relaxed), 0);
        // The cache works again afterwards.
        let nd = req(3, NFSPROC_WRITE, [10, 0, 2, 9], true);
        assert_eq!(nfsrv_getcache(&nd, &slp, &mut rep), RC_DOIT);
        assert_eq!(lru_len(), 1);
    }
}
/* </TESTS> */
