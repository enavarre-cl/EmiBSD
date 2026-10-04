//! Host tests for `nfs_socket.c`: the RTT estimator, the socket flag locks, `nfs_sigintr`,
//! `nfs_realign` on a misaligned chain, the reply header of `nfs_rephead`, the request parse
//! of `nfs_getreq` and the record marking of `nfsrv_getstream`.

use std::boxed::Box;
use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::tests::setup;
use crate::nfs::nfs_subs::tests::{bytes, chain, lens};
use crate::nfs::nfsproto::{NFSPROC_GETATTR, NFSPROC_READ, NFSPROC_SETATTR};
use crate::nfs::rpcv2::AUTH_BADCRED;
use crate::sys::param::ALIGNBYTES;

/// A mount that lives for the rest of the test run.
fn leak_mount() -> &'static NfsMount {
    Box::leak(Box::new(NfsMount::new()))
}

/// A request of procedure `procnum` on `nmp`.
fn leak_req(nmp: &'static NfsMount, procnum: usize) -> &'static NfsReq {
    let rep = Box::leak(Box::new(NfsReq::new()));
    rep.r_nmp.set(Some(nmp));
    rep.r_procnum.set(procnum);
    rep
}

/// The XDR words `w` as wire bytes.
fn wire(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|v| v.to_be_bytes()).collect()
}

#[test]
fn rtt_estimator_follows_the_c_arithmetic() {
    let _g = setup();
    let ticks = NFS_TICKS.swap(1, Relaxed);

    let nmp = leak_mount();
    nfs_init_rtt(nmp);
    assert!(nmp.nm_srtt.iter().all(|t| t.get() == nfs_initrtt()));
    assert!(nmp.nm_sdrtt.iter().all(|t| t.get() == 0));

    // GETATTR uses timer 1, index 0: A + 2D.
    nmp.nm_srtt[0].set(800);
    nmp.nm_sdrtt[0].set(80);
    let rep = leak_req(nmp, NFSPROC_GETATTR);
    rep.r_rtt.set(99);
    nfs_update_rtt(rep);
    // t1 = 100 - 800/8 = 0; srtt stays; t1 = 0 - 80/4 = -20.
    assert_eq!(nmp.nm_srtt[0].get(), 800);
    assert_eq!(nmp.nm_sdrtt[0].get(), 60);
    let rto = nfs_estimate_rto(nmp, NFSPROC_GETATTR);
    assert_eq!(rto, (803 >> 2) + (61 >> 1));
    assert!(rto > nfs_minrto() && rto < nfs_maxrto());

    // A negative error is folded: t1 = 1 - 800/8 = -99, srtt 701, |t1| - 60/4 = 84.
    rep.r_rtt.set(0);
    nfs_update_rtt(rep);
    assert_eq!(nmp.nm_srtt[0].get(), 701);
    assert_eq!(nmp.nm_sdrtt[0].get(), 144);

    // READ (timer 3, index 2): A + 4D, clamped to NFS_MAXRTO.
    nmp.nm_srtt[2].set(1 << 20);
    assert_eq!(nfs_estimate_rto(nmp, NFSPROC_READ), nfs_maxrto());
    nmp.nm_srtt[2].set(0);
    nmp.nm_sdrtt[2].set(0);
    assert_eq!(nfs_estimate_rto(nmp, NFSPROC_READ), nfs_minrto());

    // Other procedures take the mount's timeout as it is.
    nmp.nm_timeo.set(7);
    assert_eq!(nfs_estimate_rto(nmp, NFSPROC_SETATTR), 7);

    NFS_TICKS.store(ticks, Relaxed);
}

#[test]
fn flag_locks_and_interrupts() {
    let nmp = leak_mount();
    let rep = leak_req(nmp, NFSPROC_GETATTR);

    nfs_sndlock(&nmp.nm_flag, Some(rep)).expect("free lock");
    assert_ne!(nmp.nm_flag.get() & NFSMNT_SNDLOCK, 0);
    nfs_sndunlock(&nmp.nm_flag);
    assert_eq!(nmp.nm_flag.get() & NFSMNT_SNDLOCK, 0);

    nfs_rcvlock(rep).expect("free lock");
    assert_ne!(nmp.nm_flag.get() & NFSMNT_RCVLOCK, 0);
    nfs_rcvunlock(&nmp.nm_flag);
    assert_eq!(nmp.nm_flag.get() & NFSMNT_RCVLOCK, 0);

    // The server's lock word works the same way without a request.
    let solock = Cell::new(0);
    nfs_sndlock(&solock, None).expect("free lock");
    assert_eq!(solock.get(), NFSMNT_SNDLOCK);
    nfs_sndunlock(&solock);
    assert_eq!(solock.get(), 0);

    assert_eq!(nfs_sigintr(nmp, Some(rep), None), Ok(()));
    nmp.nm_flag.set(NFSMNT_INT);
    assert_eq!(nfs_sigintr(nmp, Some(rep), None), Ok(()));
    rep.r_flags.set(R_SOFTTERM);
    assert_eq!(nfs_sigintr(nmp, Some(rep), None), Err(Errno::EINTR));
    // A terminated request cannot take a busy lock either.
    nmp.nm_flag.set(NFSMNT_INT | NFSMNT_SNDLOCK);
    assert_eq!(nfs_sndlock(&nmp.nm_flag, Some(rep)), Err(Errno::EINTR));
}

/// Eight-byte alignment, the host's `ALIGNED_POINTER(x, void *)` on the real machines.
fn strict(x: usize) -> bool {
    x % 8 == 0
}

#[test]
fn realign_copies_a_misaligned_chain() {
    let _g = setup();
    assert_eq!(ALIGNBYTES, 7);

    let data: Vec<u8> = (0..29).collect();
    // An aligned first mbuf is kept; the misaligned rest is copied.
    let head = chain(&[&data[..8], &data[8..18], &data[18..21], &data[21..]]);
    let second = head.m_next().get();
    let mut pm = Some(head);
    let before = NFS_REALIGN_COUNT.load(Relaxed);
    realign(&mut pm, strict);
    assert_eq!(NFS_REALIGN_COUNT.load(Relaxed), before + 1);
    let head2 = pm.expect("a chain");
    assert!(ptr::eq(head2, head));
    assert!(!ptr::eq(
        head.m_next().get().expect("rest"),
        second.expect("old rest")
    ));
    assert_eq!(bytes(head2), data);
    let l = lens(head2);
    assert!(l[..l.len() - 1].iter().all(|&n| strict(n)), "{l:?}");
    m_freem(head2);

    // An aligned chain is left alone.
    let head = chain(&[&data[..8], &data[8..24]]);
    let mut pm = Some(head);
    let before = NFS_REALIGN_COUNT.load(Relaxed);
    realign(&mut pm, strict);
    assert_eq!(NFS_REALIGN_COUNT.load(Relaxed), before);
    assert_eq!(lens(pm.expect("chain")), [8, 16]);
    m_freem(pm);

    // A misaligned first mbuf replaces the head.
    let head = chain(&[&data[..5], &data[5..]]);
    let mut pm = Some(head);
    realign(&mut pm, strict);
    let head2 = pm.expect("a chain");
    assert!(!ptr::eq(head2, head));
    assert_eq!(bytes(head2), data);
    m_freem(head2);
}

#[test]
fn rephead_builds_the_reply_header() {
    let _g = setup();
    let mut nd = NfsrvDescript::new();
    nd.nd_retxid = 0x1234;
    let xid = 0x1234u32;

    let reply = |nd: &NfsrvDescript, err: i32| -> Vec<u8> {
        let (mreq, _mb) = nfs_rephead(0, nd, None, err).expect("reply");
        let b = bytes(mreq);
        m_freem(mreq);
        b
    };

    // Success: accepted, null verifier, status 0, then the NFS status word 0.
    assert_eq!(reply(&nd, 0), wire(&[xid, 1, 0, 0, 0, 0, 0]));
    // A void reply has no NFS status word.
    assert_eq!(reply(&nd, NFSERR_RETVOID), wire(&[xid, 1, 0, 0, 0, 0]));
    assert_eq!(
        reply(&nd, Errno::EPROGUNAVAIL.as_i32()),
        wire(&[xid, 1, 0, 0, 0, 1])
    );
    assert_eq!(
        reply(&nd, Errno::EPROGMISMATCH.as_i32()),
        wire(&[xid, 1, 0, 0, 0, 2, 2, 3])
    );
    assert_eq!(
        reply(&nd, Errno::EPROCUNAVAIL.as_i32()),
        wire(&[xid, 1, 0, 0, 0, 3])
    );
    assert_eq!(
        reply(&nd, Errno::EBADRPC.as_i32()),
        wire(&[xid, 1, 0, 0, 0, 4])
    );
    // Denied: RPC version mismatch (low and high version), or an authentication error.
    assert_eq!(
        reply(&nd, Errno::ERPCMISMATCH.as_i32()),
        wire(&[xid, 1, 1, 0, 2, 2])
    );
    assert_eq!(
        reply(&nd, NFSERR_AUTHERR | AUTH_BADCRED as i32),
        wire(&[xid, 1, 1, 1, 1])
    );
}

/// A version `vers` call of procedure `proc_` with an AUTH_UNIX credential of uid 1000,
/// gid 10 and `groups`, as an mbuf chain cut into `cut`-byte mbufs.
fn call(vers: u32, proc_: u32, groups: &[u32], cut: usize) -> &'static Mbuf {
    let mut w = std::vec![77, 0, 2, 100_003, vers, proc_, 1];
    let cred_len = 5 * 4 + 8 + 4 * groups.len() as u32;
    w.extend([cred_len, 0, 5]);
    let mut b = wire(&w);
    b.extend(b"host\0\0\0\0");
    let mut w = std::vec![1000, 10, groups.len() as u32];
    w.extend(groups);
    w.extend([0, 0]); // AUTH_NULL verifier
    b.extend(wire(&w));
    let parts: Vec<&[u8]> = b.chunks(cut).collect();
    // `chain` copies the parts, so the vector may go.
    chain(&parts)
}

/// A descriptor dissecting `m` from its start.
fn descript(m: &'static Mbuf) -> NfsrvDescript {
    let mut nd = NfsrvDescript::new();
    nd.nd_mrep = Some(m);
    nd.nd_md = Some(m);
    nd.nd_dpos = mtod::<u8>(m);
    nd
}

#[test]
fn getreq_parses_an_auth_unix_call() {
    let _g = setup();

    let mut nd = descript(call(3, 1, &[20, 30], 12));
    nfs_getreq(&mut nd, None, true).expect("a call");
    assert_eq!(nd.nd_retxid, 77);
    assert_eq!(nd.nd_repstat, 0);
    assert_eq!(nd.nd_flag, ND_NFSV3);
    assert_eq!(nd.nd_procnum, NFSPROC_GETATTR);
    assert_eq!(nd.nd_cr.cr_uid.get(), 1000);
    assert_eq!(nd.nd_cr.cr_gid.get(), 10);
    assert_eq!(nd.nd_cr.cr_ngroups.get(), 2);
    assert_eq!(nd.nd_cr.cr_groups[1].get(), 30);
    m_freem(nd.nd_mrep);

    // Version 2 procedure numbers are mapped to version 3's (v2 READ is 6, v3 READ 6;
    // v2 STATFS 17 is v3 FSSTAT 18).
    let mut nd = descript(call(2, 17, &[], 7));
    nfs_getreq(&mut nd, None, true).expect("a call");
    assert_eq!(nd.nd_flag, 0);
    assert_eq!(nd.nd_procnum, NFSV3_PROCID[17]);
    m_freem(nd.nd_mrep);

    // RPC errors are reported in nd_repstat with the NOOP procedure.
    let mut nd = descript(call(4, 1, &[], 40));
    nfs_getreq(&mut nd, None, true).expect("a call");
    assert_eq!(nd.nd_repstat, Errno::EPROGMISMATCH.as_i32());
    assert_eq!(nd.nd_procnum, NFSPROC_NOOP);
    m_freem(nd.nd_mrep);

    let mut nd = descript(call(2, 18, &[], 40));
    nfs_getreq(&mut nd, None, true).expect("a call");
    assert_eq!(nd.nd_repstat, Errno::EPROCUNAVAIL.as_i32());
    m_freem(nd.nd_mrep);

    // A reply instead of a call is garbage; the request is freed.
    let m = call(3, 1, &[], 40);
    // SAFETY: the second word of the first mbuf's data (40 bytes long).
    unsafe { mtod::<u8>(m).add(7).write(1) };
    let mut nd = descript(m);
    assert_eq!(nfs_getreq(&mut nd, None, true), Err(Errno::EBADRPC));
    assert!(nd.nd_mrep.is_none());
}

#[cfg(feature = "nfsserver")]
mod stream {
    use super::*;

    /// A server socket that lives for the rest of the test run.
    fn leak_slp() -> &'static NfssvcSock {
        Box::leak(Box::new(NfssvcSock::new()))
    }

    /// The records queued on `slp`, each as bytes.
    fn records(slp: &NfssvcSock) -> Vec<Vec<u8>> {
        let mut v = Vec::new();
        let mut r = slp.ns_rec.get();
        while let Some(m) = r {
            v.push(bytes(m));
            r = m.m_nextpkt().get();
        }
        v
    }

    #[test]
    fn getstream_splits_records_and_joins_fragments() {
        let _g = setup();
        let rec1: Vec<u8> = (1..=8).collect();
        let frag_a: Vec<u8> = (11..=14).collect();
        let frag_b: Vec<u8> = (15..=18).collect();
        let mut s = Vec::new();
        s.extend(0x8000_0008u32.to_be_bytes());
        s.extend(&rec1);
        s.extend(4u32.to_be_bytes()); // not the last fragment
        s.extend(&frag_a);
        s.extend(0x8000_0004u32.to_be_bytes());
        s.extend(&frag_b);
        s.extend(0x8000_000cu32.to_be_bytes());
        s.extend([21, 22, 23, 24, 25]); // 5 of 12 bytes so far

        let parts: Vec<&[u8]> = s.chunks(3).collect();
        let raw = chain(&parts);
        let slp = leak_slp();
        slp.ns_raw.set(Some(raw));
        let mut last = raw;
        while let Some(n) = last.m_next().get() {
            last = n;
        }
        slp.ns_rawend.set(Some(last));
        slp.ns_cc.set(s.len() as i32);

        assert_eq!(nfsrv_getstream(slp, M_WAIT), Ok(()));
        assert_eq!(slp.ns_flag.get() & SLP_GETSTREAM, 0);
        let mut rec2 = frag_a.clone();
        rec2.extend(&frag_b);
        assert_eq!(records(slp), [rec1, rec2]);
        assert!(slp.ns_frag.get().is_none());
        assert_eq!(slp.ns_reclen.get(), 12);
        assert_eq!(slp.ns_cc.get(), 5);
        assert_eq!(
            bytes(slp.ns_raw.get().expect("partial record")),
            [21, 22, 23, 24, 25]
        );
        assert_ne!(slp.ns_flag.get() & SLP_LASTFRAG, 0);

        // The record mark of a record too big for NFS disconnects.
        let big = (0x8000_0000u32 | (NFS_MAXPACKET as u32 + 1)).to_be_bytes();
        let slp = leak_slp();
        slp.ns_raw.set(Some(chain(&[&big[..2], &big[2..]])));
        slp.ns_cc.set(4);
        assert_eq!(nfsrv_getstream(slp, M_WAIT), Err(Errno::EPERM));
        assert_eq!(slp.ns_flag.get() & SLP_GETSTREAM, 0);
    }
}
