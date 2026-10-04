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

#[cfg(test)]
mod tests;
