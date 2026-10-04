//! Host tests for `nfs_srvcache.c`: the cache's three answers (drop, reply, do it), the LRU
//! recycling, the hash lookup by transaction id, procedure and client address, and the
//! clean-up.

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
