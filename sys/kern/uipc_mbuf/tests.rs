//! Host tests for the mbuf allocator, the chain operations, `mbuf_list`/`mbuf_queue` and the
//! packet tags (`uipc_mbuf2.rs`), over real memory (see `subr_pool/tests.rs` for the setup).

use std::string::String;
use std::sync::{Mutex as StdMutex, MutexGuard};
use std::vec::Vec;
use std::{assert, assert_eq, format, vec};

use super::*;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::kern::uipc_mbuf2::{
    m_pulldown, m_tag_delete, m_tag_find, m_tag_first, m_tag_get, m_tag_next, m_tag_prepend,
};
use crate::sys::mbuf::{
    MT_HEADER, PACKET_TAG_DLT, PACKET_TAG_GRE, PACKET_TAG_TUNNEL, ml_empty, mq_empty, mq_full,
};

/// Real memory, the console (so that a kernel panic in a test says why) and a fresh `mbinit`.
fn setup() -> MutexGuard<'static, ()> {
    let guard = setup_real_memory();
    crate::machine::cons::consinit();
    // mbinit registers its two free functions again.
    NUM_EXTFREE_FNS.store(0, Ordering::Relaxed);
    mbinit();
    // The host's test memory lies above amd64's 4 GiB DMA constraint, which the host double
    // mirrors; lift it as mbuf_dma_64bit_enable would.
    m_pool_noconstraints();
    guard
}

/// The bytes the tests write: position `i` holds a value that differs from its neighbours.
fn pattern(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 7 + 3) as u8).collect()
}

/// The chain's data length.
fn chain_len(m: &Mbuf) -> usize {
    let mut len = 0;
    let mut n = Some(m);
    while let Some(nn) = n {
        len += nn.m_len().get() as usize;
        n = nn.m_next().get();
    }
    len
}

/// The number of mbufs in the chain.
fn chain_count(m: &Mbuf) -> usize {
    let mut count = 0;
    let mut n = Some(m);
    while let Some(nn) = n {
        count += 1;
        n = nn.m_next().get();
    }
    count
}

/// The chain's data.
fn contents(m: &Mbuf) -> Vec<u8> {
    let mut v = vec![0u8; chain_len(m)];
    m_copydata(m, 0, &mut v);
    v
}

/// A packet of mbufs holding `lens` bytes each, filled with `pattern`.
fn packet(lens: &[usize]) -> &'static Mbuf {
    let data = pattern(lens.iter().sum());
    let mut off = 0;
    let mut top: Option<&'static Mbuf> = None;
    let mut last: Option<&'static Mbuf> = None;
    for &len in lens {
        let m = if top.is_none() {
            m_gethdr(M_DONTWAIT, MT_DATA).expect("a header mbuf")
        } else {
            m_get(M_DONTWAIT, MT_DATA).expect("an mbuf")
        };
        if len > m_trailingspace(m) as usize {
            assert!(mclgetl(m, M_DONTWAIT, len as u32).is_some(), "a cluster");
        }
        m.m_len().set(len as u32);
        m_copyback(m, 0, &data[off..off + len], M_DONTWAIT).expect("copyback");
        off += len;
        match last {
            None => top = Some(m),
            Some(last) => last.m_next().set(Some(m)),
        }
        last = Some(m);
    }
    let top = top.expect("at least one mbuf");
    top.m_pkthdr().len.set(off as i32);
    top
}

#[test]
fn get_gethdr_and_free() {
    let _g = setup();

    let m = m_get(M_DONTWAIT, MT_DATA).expect("an mbuf");
    assert_eq!(m.m_data().get(), m.m_dat());
    assert_eq!(m.m_flags().get(), 0);
    assert_eq!(m.m_len().get(), 0);
    assert_eq!(i32::from(m.m_type().get()), MT_DATA);
    assert_eq!(m_leadingspace(m), 0);
    assert_eq!(m_trailingspace(m), MLEN as i32);

    let h = m_gethdr(M_WAIT, MT_HEADER).expect("a header mbuf");
    assert_eq!(h.m_flags().get(), M_PKTHDR);
    assert_eq!(h.m_data().get(), h.m_pktdat());
    assert_eq!(h.m_pkthdr().pf.prio.get(), IFQ_DEFPRIO);
    assert_eq!(h.m_pkthdr().len.get(), 0);
    assert!(h.m_pkthdr().ph_tags.first().is_none());
    assert_eq!(m_trailingspace(h), MHLEN as i32);
    assert_eq!(MBPOOL.pr_nout.get(), 2);

    h.m_next().set(Some(m));
    assert!(ptr::eq(m_free(h).expect("the next mbuf"), m));
    assert!(m_free(m).is_none());
    assert!(m_free(None).is_none());
    assert_eq!(MBPOOL.pr_nout.get(), 0);

    let c = m_getclr(M_DONTWAIT, MT_DATA).expect("a zeroed mbuf");
    c.m_len().set(MLEN as u32);
    assert_eq!(contents(c), vec![0u8; MLEN]);
    m_freem(c);
}

#[test]
fn cluster_pools_have_the_c_names_and_sizes() {
    let _g = setup();

    let names: Vec<&str> = (0..MCLSIZES.len()).map(mclname).collect();
    assert_eq!(
        names,
        [
            "mcl2k", "mcl2k2", "mcl4k", "mcl8k", "mcl9k128", "mcl12k", "mcl16k", "mcl64k"
        ]
    );
    for (pp, &size) in MCLPOOLS.iter().zip(MCLSIZES.iter()) {
        assert!(pp.pr_size.get() >= size);
    }
    assert!(ptr::eq(m_clpool(1500).expect("a pool"), &MCLPOOLS[0]));
    assert!(ptr::eq(m_clpool(2049).expect("a pool"), &MCLPOOLS[1]));
    assert!(ptr::eq(m_clpool(9000).expect("a pool"), &MCLPOOLS[4]));
    assert!(m_clpool(64 * 1024 + 1).is_none());
    assert_eq!(nmbclust_update(0), Err(Errno::ERANGE));
    assert_eq!(nmbclust_update(1024), Ok(()));
    assert_eq!(
        MBUF_MEM_LIMIT.load(Ordering::Relaxed),
        1024 * MCLBYTES as u64
    );
    nmbclust_update(crate::sys::param::NMBCLUSTERS as i64).expect("the default limit");
}

#[test]
fn clusters_come_and_go() {
    let _g = setup();

    let m = m_clget(None, M_DONTWAIT, 9000).expect("a cluster packet");
    assert_eq!(
        m.m_flags().get() & (M_EXT | M_EXTWR | M_PKTHDR),
        M_EXT | M_EXTWR | M_PKTHDR
    );
    assert_eq!(m.m_ext().ext_size.get(), MCLPOOLS[4].pr_size.get());
    assert_eq!(m.m_data().get(), m.m_ext().ext_buf.get());
    assert!(!m_readonly(m));
    assert_eq!(MCLPOOLS[4].pr_nout.get(), 1);
    assert!(MBUF_MEM_ALLOC.load(Ordering::Relaxed) > 0);
    assert!(m_pool_used() < 100);
    m_freem(m);
    assert_eq!(MCLPOOLS[4].pr_nout.get(), 0);
    assert_eq!(MBPOOL.pr_nout.get(), 0);
}

#[test]
fn copydata_and_copyback_round_trip() {
    let _g = setup();

    let m = packet(&[100, 50, 200]);
    let data = pattern(350);
    assert_eq!(contents(m), data);

    let mut part = vec![0u8; 100];
    m_copydata(m, 120, &mut part);
    assert_eq!(part, &data[120..220]);

    // across the first two mbufs
    let patch: Vec<u8> = (0..40).map(|i| 200 + i as u8).collect();
    m_copyback(m, 90, &patch, M_DONTWAIT).expect("copyback");
    let mut want = data.clone();
    want[90..130].copy_from_slice(&patch);
    assert_eq!(contents(m), want);
    assert_eq!(m.m_pkthdr().len.get(), 350);

    // past the end: the gap is zeroed and the packet grows
    let tail = pattern(300);
    m_copyback(m, 360, &tail, M_DONTWAIT).expect("copyback past the end");
    want.extend_from_slice(&[0u8; 10]);
    want.extend_from_slice(&tail);
    assert_eq!(contents(m), want);
    assert_eq!(m.m_pkthdr().len.get(), 660);
    m_calchdrlen(m);
    assert_eq!(m.m_pkthdr().len.get(), 660);

    m_freem(m);
    assert_eq!(MBPOOL.pr_nout.get(), 0);
    assert!(MCLPOOLS.iter().all(|pp| pp.pr_nout.get() == 0));
}

#[test]
fn adj_trims_the_head_and_the_tail() {
    let _g = setup();

    let m = packet(&[100, 50, 200]);
    let data = pattern(350);

    m_adj(m, 120);
    assert_eq!(m.m_pkthdr().len.get(), 230);
    assert_eq!(m.m_len().get(), 0);
    assert_eq!(m.m_next().get().expect("second").m_len().get(), 30);
    assert_eq!(contents(m), &data[120..]);

    // within the last mbuf only
    m_adj(m, -10);
    assert_eq!(m.m_pkthdr().len.get(), 220);
    assert_eq!(contents(m), &data[120..340]);

    // across mbufs
    m_adj(m, -200);
    assert_eq!(m.m_pkthdr().len.get(), 20);
    assert_eq!(contents(m), &data[120..140]);

    m_adj(None, 5);
    m_freem(m);
    assert_eq!(MBPOOL.pr_nout.get(), 0);
}

#[test]
fn pullup_makes_the_head_contiguous() {
    let _g = setup();

    let data = pattern(260);
    let m = packet(&[10, 50, 200]);
    let m = m_pullup(m, 40).expect("pulled up");
    assert!(m.m_len().get() >= 40);
    assert_eq!(m.m_flags().get() & M_PKTHDR, M_PKTHDR);
    assert_eq!(m.m_pkthdr().len.get(), 260);
    assert_eq!(contents(m), data);

    // more than a header mbuf holds: a new mbuf with a cluster leads the chain
    let m = m_pullup(m, 200).expect("pulled up into a cluster");
    assert!(m.m_len().get() >= 200);
    assert_eq!(m.m_flags().get() & M_EXT, M_EXT);
    assert_eq!(m.m_pkthdr().len.get(), 260);
    assert_eq!(contents(m), data);

    // longer than the chain: the chain is freed
    assert!(m_pullup(m, 1000).is_none());
    assert_eq!(MBPOOL.pr_nout.get(), 0);
    assert!(MCLPOOLS.iter().all(|pp| pp.pr_nout.get() == 0));
}

#[test]
fn split_plain_and_shared_clusters() {
    let _g = setup();

    let data = pattern(350);
    let m = packet(&[100, 50, 200]);
    let tail = m_split(m, 120, M_DONTWAIT).expect("a tail");
    assert_eq!(m.m_pkthdr().len.get(), 120);
    assert_eq!(contents(m), &data[..120]);
    assert_eq!(tail.m_flags().get() & M_PKTHDR, M_PKTHDR);
    assert_eq!(tail.m_pkthdr().len.get(), 230);
    assert_eq!(contents(tail), &data[120..]);
    m_freem(m);
    m_freem(tail);

    // a cluster is shared, not copied
    let data = pattern(1000);
    let m = packet(&[1000]);
    assert_eq!(m.m_flags().get() & M_EXT, M_EXT);
    let tail = m_split(m, 300, M_DONTWAIT).expect("a tail");
    assert_eq!(tail.m_flags().get() & M_PKTHDR, M_PKTHDR);
    // the header mbuf of the tail points into the same cluster
    let shared = tail;
    assert_eq!(chain_count(tail), 1);
    assert_eq!(shared.m_ext().ext_buf.get(), m.m_ext().ext_buf.get());
    assert_eq!(
        shared.m_data().get() as usize - m.m_data().get() as usize,
        300
    );
    assert!(m_readonly(m) && m_readonly(shared));
    assert_eq!(m_trailingspace(m), 0);
    assert_eq!(contents(m), &data[..300]);
    assert_eq!(contents(tail), &data[300..]);
    assert_eq!(MCLPOOLS[0].pr_nout.get(), 1);
    m_freem(m);
    assert_eq!(
        MCLPOOLS[0].pr_nout.get(),
        1,
        "the tail still holds the cluster"
    );
    assert!(!m_readonly(shared), "the last reference is writable again");
    m_freem(tail);
    assert_eq!(MCLPOOLS[0].pr_nout.get(), 0);
    assert_eq!(M_EXT_REFS_POOL.pr_nout.get(), 0);
    assert_eq!(MBPOOL.pr_nout.get(), 0);
}

#[test]
fn prepend_uses_leading_space_then_a_new_mbuf() {
    let _g = setup();

    let m = m_gethdr(M_DONTWAIT, MT_DATA).expect("a header mbuf");
    m_align(m, 20);
    m.m_len().set(20);
    m_copyback(m, 0, &pattern(20), M_DONTWAIT).expect("copyback");
    m.m_pkthdr().len.set(20);
    let room = m_leadingspace(m);
    assert_eq!(room, (MHLEN - 24) as i32);

    let m = m_prepend(m, 8, M_DONTWAIT).expect("prepended in place");
    assert_eq!(chain_count(m), 1);
    assert_eq!(m.m_len().get(), 28);
    assert_eq!(m.m_pkthdr().len.get(), 28);

    let big = m_leadingspace(m) + 1;
    let first = m;
    let m = m_prepend(m, big, M_DONTWAIT).expect("prepended in a new mbuf");
    assert!(!ptr::eq(m, first));
    assert!(ptr::eq(m.m_next().get().expect("the old head"), first));
    assert_eq!(m.m_flags().get() & M_PKTHDR, M_PKTHDR);
    assert_eq!(first.m_flags().get() & M_PKTHDR, 0);
    assert_eq!(m.m_len().get(), big as u32);
    assert_eq!(m.m_pkthdr().len.get(), 28 + big);
    let mut tail = vec![0u8; 20];
    m_copydata(m, 8 + big, &mut tail);
    assert_eq!(tail, pattern(20));
    m_freem(m);
    assert_eq!(MBPOOL.pr_nout.get(), 0);
}

#[test]
fn cat_copies_small_data_and_links_large() {
    let _g = setup();

    let a = packet(&[30]);
    let b = m_get(M_DONTWAIT, MT_DATA).expect("an mbuf");
    b.m_len().set(10);
    m_copyback(b, 0, &[9u8; 10], M_DONTWAIT).expect("copyback");
    m_cat(a, Some(b));
    assert_eq!(chain_count(a), 1, "the data was copied, b freed");
    let mut want = pattern(30);
    want.extend_from_slice(&[9u8; 10]);
    assert_eq!(contents(a), want);

    let big = packet(&[200]);
    m_removehdr(big);
    m_cat(a, Some(big));
    assert_eq!(chain_count(a), 2, "too big to copy: linked");
    m_freem(a);
    assert_eq!(MBPOOL.pr_nout.get(), 0);
}

#[test]
fn copym_copies_and_shares() {
    let _g = setup();

    let data = pattern(1250);
    let m = packet(&[100, 150, 1000]);
    let copy = m_copym(m, 0, M_COPYALL, M_DONTWAIT).expect("a copy");
    assert_eq!(copy.m_flags().get() & M_PKTHDR, M_PKTHDR);
    assert_eq!(copy.m_pkthdr().len.get(), 1250);
    assert_eq!(contents(copy), data);
    assert_eq!(MCLPOOLS[0].pr_nout.get(), 1, "the cluster is shared");

    let part = m_copym(m, 50, 200, M_DONTWAIT).expect("a partial copy");
    assert_eq!(part.m_flags().get() & M_PKTHDR, 0);
    assert_eq!(contents(part), &data[50..250]);

    m_freem(m);
    m_freem(part);
    assert_eq!(MCLPOOLS[0].pr_nout.get(), 1);
    m_freem(copy);
    assert_eq!(MCLPOOLS[0].pr_nout.get(), 0);
    assert_eq!(MBPOOL.pr_nout.get(), 0);
}

#[test]
fn defrag_and_dup_pkt() {
    let _g = setup();

    let data = pattern(600);
    let m = packet(&[100, 300, 200]);
    m_defrag(m, M_DONTWAIT).expect("defragmented");
    assert_eq!(chain_count(m), 1);
    assert_eq!(m.m_flags().get() & M_EXT, M_EXT);
    assert_eq!(m.m_pkthdr().len.get(), 600);
    assert_eq!(contents(m), data);

    let d = m_dup_pkt(m, 2, M_DONTWAIT).expect("a duplicate");
    assert_eq!(contents(d), data);
    assert_eq!(d.m_pkthdr().len.get(), 600);
    assert_eq!(
        d.m_data().get() as usize - d.m_ext().ext_buf.get() as usize,
        2
    );

    m_freem(m);
    m_freem(d);
    assert_eq!(MBPOOL.pr_nout.get(), 0);
    assert!(MCLPOOLS.iter().all(|pp| pp.pr_nout.get() == 0));
}

#[test]
fn getptr_apply_and_devget() {
    let _g = setup();

    let data = pattern(350);
    let m = packet(&[100, 50, 200]);
    let (n, off) = m_getptr(m, 120).expect("inside");
    assert_eq!(n.m_len().get(), 50);
    assert_eq!(off, 20);
    let (n, off) = m_getptr(m, 350).expect("the end");
    assert_eq!((n.m_len().get(), off), (200, 200));
    assert!(m_getptr(m, 351).is_none());

    let mut sum = 0u64;
    m_apply(m, 90, 100, |b| {
        sum += b.iter().map(|&x| u64::from(x)).sum::<u64>();
        Ok(())
    })
    .expect("applied");
    assert_eq!(
        sum,
        data[90..190].iter().map(|&x| u64::from(x)).sum::<u64>()
    );
    assert_eq!(
        m_apply(m, 0, 10, |_| Err(Errno::EINVAL)),
        Err(Errno::EINVAL)
    );
    m_freem(m);

    MAX_LINKHDR.store(16, Ordering::Relaxed);
    let small = m_devget(&data[..60], 0).expect("a small packet");
    assert_eq!(small.m_pkthdr().len.get(), 60);
    assert_eq!(m_leadingspace(small), 16);
    assert_eq!(contents(small), &data[..60]);
    m_freem(small);

    let frame: Vec<u8> = pattern(1514);
    let big = m_devget(&frame, 2).expect("a cluster packet");
    assert_eq!(big.m_flags().get() & M_EXT, M_EXT);
    assert_eq!(contents(big), frame);
    m_freem(big);
    MAX_LINKHDR.store(0, Ordering::Relaxed);
    assert_eq!(MBPOOL.pr_nout.get(), 0);
}

#[test]
fn pulldown_and_makespace() {
    let _g = setup();

    let data = pattern(260);
    let m = packet(&[10, 50, 200]);
    let n = m_pulldown(m, 5, 30, None).expect("pulled down");
    assert!(n.m_len().get() >= 30);
    let mut got = vec![0u8; 30];
    m_copydata(n, 0, &mut got);
    assert_eq!(got, &data[5..35]);
    assert_eq!(contents(m), data);

    let mut off = -1;
    let n = m_pulldown(m, 100, 20, Some(&mut off)).expect("already contiguous");
    assert!(n.m_len().get() as i32 - off >= 20);
    m_copydata(n, off, &mut got[..20]);
    assert_eq!(&got[..20], &data[100..120]);
    m_freem(m);

    let m = packet(&[40]);
    let (n, off) = m_makespace(m, 20, 8).expect("space");
    assert!(ptr::eq(n, m));
    assert_eq!(m.m_pkthdr().len.get(), 48);
    let all = contents(m);
    assert_eq!(&all[..off as usize], &pattern(40)[..20]);
    assert_eq!(&all[off as usize + 8..], &pattern(40)[20..]);
    m_freem(m);
    assert_eq!(MBPOOL.pr_nout.get(), 0);
}

#[test]
fn mbuf_lists_and_queues() {
    let _g = setup();

    let ml = MbufList::new();
    assert!(ml_empty(&ml));
    let pkts: Vec<&'static Mbuf> = (0..3).map(|_| packet(&[10])).collect();
    for &p in &pkts {
        ml_enqueue(&ml, p);
    }
    assert_eq!(ml_len(&ml), 3);
    assert_eq!(ml_hdatalen(&ml), 10);
    assert_eq!(ml.iter().count(), 3);
    assert!(ptr::eq(ml_dequeue(&ml).expect("first"), pkts[0]));
    assert_eq!(ml_len(&ml), 2);

    let other = MbufList::new();
    ml_enqueue(&other, pkts[0]);
    ml_enlist(&ml, &other);
    assert!(ml_empty(&other));
    let order: Vec<*const Mbuf> = ml.iter().map(ptr::from_ref).collect();
    assert_eq!(
        order,
        [pkts[1], pkts[2], pkts[0]].map(ptr::from_ref).to_vec()
    );
    assert_eq!(ml_purge(&ml), 3);
    assert!(ml_empty(&ml));
    assert_eq!(MBPOOL.pr_nout.get(), 0);

    let mq = MbufQueue::new(0, IPL_NET);
    mq_init(&mq, 2, IPL_NET);
    assert!(mq_empty(&mq));
    assert!(!mq_enqueue(&mq, packet(&[20])));
    assert!(!mq_enqueue(&mq, packet(&[30])));
    assert!(mq_full(&mq));
    assert!(mq_enqueue(&mq, packet(&[40])), "dropped when full");
    assert_eq!(mq_drops(&mq), 1);
    assert_eq!(mq_hdatalen(&mq), 20);
    assert!(mq_push(&mq, packet(&[50])), "the oldest is dropped");
    assert_eq!(mq_drops(&mq), 2);
    assert_eq!(mq_hdatalen(&mq), 30);

    let batch = MbufList::new();
    ml_enqueue(&batch, packet(&[60]));
    assert_eq!(mq_enlist(&mq, &batch), 1, "full: the batch is dropped");
    mq_set_maxlen(&mq, 4);
    ml_enqueue(&batch, packet(&[70]));
    assert_eq!(mq_enlist(&mq, &batch), 0);
    assert_eq!(mq_len(&mq), 3);

    let out = MbufList::new();
    mq_delist(&mq, &out);
    assert!(mq_empty(&mq));
    assert_eq!(ml_len(&out), 3);
    let m = ml_dequeue(&out).expect("a packet");
    assert_eq!(m.m_pkthdr().len.get(), 30);
    m_freem(m);
    ml_purge(&out);
    assert!(mq_dequeue(&mq).is_none());
    assert!(!mq_enqueue(&mq, packet(&[80])));
    assert_eq!(mq_purge(&mq), 1);
    assert_eq!(MBPOOL.pr_nout.get(), 0);

    let name = [IFQCTL_LEN, 0];
    let mut len = 0;
    assert_eq!(
        sysctl_mq(&name, 0, &mut len, 0, 0, &mq),
        Err(Errno::ENOTDIR)
    );
    assert_eq!(
        sysctl_mq(&[99], 0, &mut len, 0, 0, &mq),
        Err(Errno::EOPNOTSUPP)
    );

    // The nodes through the kern_sysctl.c helpers: a length, a new maximum, the drops.
    let mut val = [0u8; 4];
    let mut len = val.len();
    let oldp = val.as_mut_ptr() as usize;
    assert_eq!(sysctl_mq(&[IFQCTL_LEN], oldp, &mut len, 0, 0, &mq), Ok(()));
    assert_eq!((len, i32::from_ne_bytes(val)), (4, mq_len(&mq) as i32));
    let newmax = 7i32.to_ne_bytes();
    let newp = newmax.as_ptr() as usize;
    assert_eq!(
        sysctl_mq(&[IFQCTL_MAXLEN], 0, &mut len, newp, 4, &mq),
        Ok(())
    );
    assert_eq!(mq.mq_maxlen.load(Ordering::Relaxed), 7);
    assert_eq!(
        sysctl_mq(&[IFQCTL_DROPS], oldp, &mut len, newp, 4, &mq),
        Err(Errno::EPERM)
    );
}

#[test]
fn tags_are_added_found_copied_and_deleted() {
    let _g = setup();

    let m = m_gethdr(M_DONTWAIT, MT_DATA).expect("a header mbuf");
    assert!(m_tag_find(m, PACKET_TAG_GRE, None).is_none());
    assert!(m_tag_get(PACKET_TAG_GRE, -1, M_DONTWAIT).is_none());

    let gre = m_tag_get(PACKET_TAG_GRE, 4, M_DONTWAIT).expect("a tag");
    // SAFETY: the tag item has PACKET_TAG_MAXSIZE bytes of data after the structure.
    unsafe { ptr::copy_nonoverlapping([1u8, 2, 3, 4].as_ptr(), gre.data(), 4) };
    m_tag_prepend(m, gre);
    let dlt = m_tag_get(PACKET_TAG_DLT, 0, M_DONTWAIT).expect("a tag");
    m_tag_prepend(m, dlt);
    let gre2 = m_tag_get(PACKET_TAG_GRE, 0, M_DONTWAIT).expect("a tag");
    m_tag_prepend(m, gre2);
    assert_eq!(
        m.m_pkthdr().ph_tagsset.get(),
        PACKET_TAG_GRE | PACKET_TAG_DLT
    );
    assert_eq!(MTAGPOOL.pr_nout.get(), 3);

    // newest first
    let first = m_tag_find(m, PACKET_TAG_GRE, None).expect("found");
    assert!(ptr::eq(first, gre2));
    let second = m_tag_find(m, PACKET_TAG_GRE, Some(first)).expect("found again");
    assert!(ptr::eq(second, gre));
    assert!(m_tag_find(m, PACKET_TAG_TUNNEL, None).is_none());
    assert!(ptr::eq(m_tag_first(m).expect("first"), gre2));
    assert!(ptr::eq(m_tag_next(m, gre2).expect("next"), dlt));

    // a duplicate header copies the chain in order
    let d = m_get(M_DONTWAIT, MT_DATA).expect("an mbuf");
    m_dup_pkthdr(d, m, M_DONTWAIT).expect("duplicated");
    assert_eq!(MTAGPOOL.pr_nout.get(), 6);
    let ids: Vec<u16> = d
        .m_pkthdr()
        .ph_tags
        .iter()
        .map(|t| t.m_tag_id.get())
        .collect();
    assert_eq!(ids, [PACKET_TAG_GRE, PACKET_TAG_DLT, PACKET_TAG_GRE]);
    let copied = m_tag_find(d, PACKET_TAG_GRE, m_tag_find(d, PACKET_TAG_GRE, None))
        .expect("the copied data tag");
    let mut bytes = [0u8; 4];
    // SAFETY: the copy has the 4 data bytes m_tag_copy wrote.
    unsafe { ptr::copy_nonoverlapping(copied.data(), bytes.as_mut_ptr(), 4) };
    assert_eq!(bytes, [1, 2, 3, 4]);
    m_freem(d);
    assert_eq!(MTAGPOOL.pr_nout.get(), 3);

    // SAFETY: `dlt` is on `m`'s list.
    unsafe { m_tag_delete(m, dlt) };
    assert_eq!(m.m_pkthdr().ph_tagsset.get(), PACKET_TAG_GRE);
    // SAFETY: `gre2` and `gre` are on `m`'s list.
    unsafe {
        m_tag_delete(m, gre2);
        m_tag_delete(m, gre);
    }
    assert_eq!(m.m_pkthdr().ph_tagsset.get(), 0);
    assert!(m_tag_find(m, PACKET_TAG_GRE, None).is_none());

    m_tag_prepend(
        m,
        m_tag_get(PACKET_TAG_TUNNEL, 8, M_DONTWAIT).expect("a tag"),
    );
    m_resethdr(m);
    assert!(m.m_pkthdr().ph_tags.first().is_none());
    assert_eq!(m.m_pkthdr().pf.prio.get(), IFQ_DEFPRIO);
    m_freem(m);
    assert_eq!(MTAGPOOL.pr_nout.get(), 0);
    assert_eq!(MBPOOL.pr_nout.get(), 0);
}

static PRINTED: StdMutex<String> = StdMutex::new(String::new());

fn collect(args: core::fmt::Arguments<'_>) {
    PRINTED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push_str(&format!("{args}"));
}

#[test]
fn ddb_printers() {
    let _g = setup();

    PRINTED.lock().unwrap_or_else(|e| e.into_inner()).clear();
    let m = packet(&[100, 2000]);
    m_print(m, collect);
    m_print_chain(Some(m), false, collect);
    m_print_packet(Some(m), false, collect);
    let out = PRINTED.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert!(out.contains("m_type: 1\tm_flags: 2<M_PKTHDR>\n"), "{out}");
    assert!(out.contains("m_pkthdr.len: 2100\n"), "{out}");
    assert!(
        out.contains(", dat, off 0, len 100, pktlen 2100, size 144\n"),
        "{out}"
    );
    assert!(out.contains(", clsize 2048\n"), "{out}");
    assert!(
        out.contains(" \\- total chain 2, len 2100, size 2192\n"),
        "{out}"
    );
    assert!(
        out.contains(", chain 2, pktlen 2100, len 2100, size 2192\n"),
        "{out}"
    );
    assert!(out.contains("\\-- total packets 1\n"), "{out}");
    m_freem(m);
}

#[test]
fn zero_clears_the_data_area() {
    let _g = setup();

    let m = packet(&[64]);
    m_zero(m);
    assert_eq!(contents(m), vec![0u8; 64]);
    m.m_flags().set(m.m_flags().get() | M_ZEROIZE);
    m_freem(m);
    assert_eq!(MBPOOL.pr_nout.get(), 0);
}
