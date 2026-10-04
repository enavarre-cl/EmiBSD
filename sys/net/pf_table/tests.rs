//! Host tests for pf's tables: creating tables (also anchored ones and their root tables),
//! adding, testing and deleting IPv4 addresses and networks through kernel buffers,
//! longest-prefix matches with negated entries, table and entry counters, a transaction
//! (`pfr_ina_begin`/`define`/`commit`) and round-robin address pools.

use std::boxed::Box;
use std::sync::MutexGuard;
use std::vec::Vec;

use super::*;
use crate::net::pf_ruleset::{PF_ANCHORS, pf_find_anchor};
use crate::net::pfvar::PfRuleset;

fn setup() -> MutexGuard<'static, ()> {
    let guard = crate::net::pf_ruleset::tests::setup();
    pfr_initialize();
    PFR_KTABLES.init();
    PFR_KTABLE_CNT.set(0);
    guard
}

/// A table description `name` in `anchor`, with `flags`.
fn table(name: &str, anchor: &str, flags: u32) -> PfrTable {
    let mut t = PfrTable::zeroed();
    strlcpy(&mut t.pfrt_name, name.as_bytes());
    strlcpy(&mut t.pfrt_anchor, anchor.as_bytes());
    t.pfrt_flags.set(flags);
    t
}

/// An IPv4 table address `a/net`, negated with `not`.
fn addr4(a: [u8; 4], net: u8, not: bool) -> PfrAddr {
    PfrAddr {
        pfra_u: pf4(a),
        pfra_af: AF_INET,
        pfra_net: net,
        pfra_not: u8::from(not),
        ..PfrAddr::default()
    }
}

/// An address's first four bytes and its feedback code.
type Feedback = ([u8; 4], u8);

fn pf4(a: [u8; 4]) -> PfAddr {
    PfAddr::from_v4(InAddr {
        s_addr: u32::from_ne_bytes(a),
    })
}

/// Adds the tables of `t` (kernel buffer), returning the count added.
fn add_tables(t: Vec<PfrTable>) -> i32 {
    let mut t = t;
    let n = t.len() as i32;
    let mut nadd = -1;
    pfr_add_tables(&PfrBuf::Kernel(&mut t), n, Some(&mut nadd), 0).expect("tables added");
    nadd
}

/// Adds `addrs` to `tbl` with feedback, returning the count and what the buffer holds
/// afterwards: each address with its feedback code. The C reports the entries in the order of
/// its `ioq`, built by inserting at the head: the reverse of the input.
fn add_addrs(tbl: &mut PfrTable, addrs: &[PfrAddr]) -> Result<(i32, Vec<Feedback>), Errno> {
    let mut v = addrs.to_vec();
    let mut nadd = -1;
    let n = v.len() as i32;
    pfr_add_addrs(
        tbl,
        &mut PfrBuf::Kernel(&mut v),
        n,
        Some(&mut nadd),
        PFR_FLAG_FEEDBACK,
    )?;
    let fb = v
        .iter()
        .map(|a| {
            let b = &a.pfra_u.addr8;
            ([b[0], b[1], b[2], b[3]], a.pfra_fback)
        })
        .collect();
    Ok((nadd, fb))
}

#[test]
fn tables_addresses_and_matches() {
    let _g = setup();

    assert_eq!(add_tables(std::vec![table("t", "", 0)]), 1);
    assert_eq!(PFR_KTABLE_CNT.get(), 1);
    // Adding it again adds nothing.
    assert_eq!(add_tables(std::vec![table("t", "", 0)]), 0);
    let mut tbl = table("t", "", 0);
    let kt = pfr_lookup_table(&tbl).expect("table");
    assert!(kt.pfrkt_flags().get() & PFR_TFLAG_ACTIVE != 0);
    assert!(ptr::eq(kt.pfrkt_rs.get().unwrap(), pf_main_ruleset()));
    assert_eq!(pf_main_ruleset().tables.get(), 1);

    let (nadd, fb) = add_addrs(
        &mut tbl,
        &[
            addr4([10, 0, 0, 0], 8, false),
            addr4([10, 1, 0, 0], 16, true),
            addr4([10, 1, 2, 3], 32, false),
            addr4([192, 168, 1, 1], 32, false),
            addr4([10, 1, 2, 3], 32, false),
        ],
    )
    .expect("added");
    assert_eq!(nadd, 4);
    assert_eq!(
        fb,
        [
            ([10, 1, 2, 3], PFR_FB_ADDED),
            ([192, 168, 1, 1], PFR_FB_ADDED),
            ([10, 1, 2, 3], PFR_FB_DUPLICATE),
            ([10, 1, 0, 0], PFR_FB_ADDED),
            ([10, 0, 0, 0], PFR_FB_ADDED),
        ]
    );
    assert_eq!(kt.pfrkt_cnt().get(), 4);

    // Longest prefix wins; the negated /16 hides the /8 under it.
    assert!(pfr_match_addr(kt, &pf4([10, 2, 3, 4]), AF_INET));
    assert!(!pfr_match_addr(kt, &pf4([10, 1, 5, 5]), AF_INET));
    assert!(pfr_match_addr(kt, &pf4([10, 1, 2, 3]), AF_INET));
    assert!(pfr_match_addr(kt, &pf4([192, 168, 1, 1]), AF_INET));
    assert!(!pfr_match_addr(kt, &pf4([192, 168, 1, 2]), AF_INET));
    assert_eq!(kt.pfrkt_match().get(), 3);
    assert_eq!(kt.pfrkt_nomatch().get(), 2);

    // An address already there, and a conflicting negation, are not added.
    let (nadd, fb) = add_addrs(
        &mut tbl,
        &[
            addr4([10, 0, 0, 0], 8, false),
            addr4([192, 168, 1, 1], 32, true),
        ],
    )
    .unwrap();
    assert_eq!(nadd, 0);
    assert_eq!(
        fb,
        [
            ([192, 168, 1, 1], PFR_FB_CONFLICT),
            ([10, 0, 0, 0], PFR_FB_NONE)
        ]
    );
    // Host bits under the mask, and unknown tables, are refused.
    assert_eq!(
        add_addrs(&mut tbl, &[addr4([10, 0, 0, 1], 8, false)]),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        add_addrs(&mut table("nope", "", 0), &[addr4([1, 2, 3, 4], 32, false)]),
        Err(Errno::ESRCH)
    );

    // pfr_tst_addrs.
    let mut v = std::vec![
        addr4([10, 1, 5, 5], 32, false),
        addr4([10, 9, 9, 9], 32, false),
        addr4([1, 2, 3, 4], 32, false),
    ];
    let mut nmatch = -1;
    pfr_tst_addrs(
        &mut tbl,
        &mut PfrBuf::Kernel(&mut v),
        3,
        Some(&mut nmatch),
        0,
    )
    .unwrap();
    assert_eq!(nmatch, 1);
    assert_eq!(v[0].pfra_fback, PFR_FB_NOTMATCH);
    assert_eq!(v[1].pfra_fback, PFR_FB_MATCH);
    assert_eq!(v[2].pfra_fback, PFR_FB_NONE);
    // With PFR_FLAG_REPLACE each address becomes the entry it matched.
    let mut v = std::vec![addr4([10, 9, 9, 9], 32, false)];
    pfr_tst_addrs(
        &mut tbl,
        &mut PfrBuf::Kernel(&mut v),
        1,
        None,
        PFR_FLAG_REPLACE,
    )
    .unwrap();
    assert_eq!(v[0].pfra_net, 8);
    assert_eq!(v[0].pfra_u.addr8[..4], [10, 0, 0, 0]);
    // Networks cannot be tested.
    let mut v = std::vec![addr4([10, 0, 0, 0], 8, false)];
    assert_eq!(
        pfr_tst_addrs(&mut tbl, &mut PfrBuf::Kernel(&mut v), 1, None, 0),
        Err(Errno::EINVAL)
    );

    // pfr_del_addrs.
    let mut v = std::vec![
        addr4([10, 1, 2, 3], 32, false),
        addr4([1, 1, 1, 1], 32, false)
    ];
    let mut ndel = -1;
    pfr_del_addrs(
        &mut tbl,
        &mut PfrBuf::Kernel(&mut v),
        2,
        Some(&mut ndel),
        PFR_FLAG_FEEDBACK,
    )
    .unwrap();
    assert_eq!(ndel, 1);
    assert_eq!(v[0].pfra_fback, PFR_FB_DELETED);
    assert_eq!(v[1].pfra_fback, PFR_FB_NONE);
    assert_eq!(kt.pfrkt_cnt().get(), 3);
    assert!(
        !pfr_match_addr(kt, &pf4([10, 1, 2, 3]), AF_INET),
        "under the negated /16"
    );

    // The size query of pfr_get_addrs (its copy is copyout(9) to a user buffer).
    let mut size = 0;
    pfr_get_addrs(&mut tbl, &mut PfrBuf::Kernel(&mut []), &mut size, 0).unwrap();
    assert_eq!(size, 3);

    // pfr_set_addrs replaces the content.
    let mut v = std::vec![
        addr4([10, 0, 0, 0], 8, false),
        addr4([172, 16, 0, 0], 12, false)
    ];
    let (mut nadd, mut ndel, mut nchange) = (-1, -1, -1);
    pfr_set_addrs(
        &mut tbl,
        &mut PfrBuf::Kernel(&mut v),
        2,
        None,
        Some(&mut nadd),
        Some(&mut ndel),
        Some(&mut nchange),
        0,
        0,
    )
    .unwrap();
    assert_eq!((nadd, ndel, nchange), (1, 2, 0));
    assert_eq!(kt.pfrkt_cnt().get(), 2);
    assert!(pfr_match_addr(kt, &pf4([172, 20, 1, 1]), AF_INET));
    assert!(!pfr_match_addr(kt, &pf4([192, 168, 1, 1]), AF_INET));

    // pfr_clr_addrs empties it.
    let mut ndel = -1;
    pfr_clr_addrs(&mut tbl, Some(&mut ndel), 0).unwrap();
    assert_eq!(ndel, 2);
    assert_eq!(kt.pfrkt_cnt().get(), 0);
    assert_eq!(PFR_KENTRY_PL[PFRKE_PLAIN as usize].pr_nout.get(), 0);

    // pfr_del_tables destroys the table.
    let mut ts = std::vec![table("t", "", 0)];
    let mut ndel = -1;
    pfr_del_tables(&PfrBuf::Kernel(&mut ts), 1, Some(&mut ndel), 0).unwrap();
    assert_eq!(ndel, 1);
    assert!(pfr_lookup_table(&tbl).is_none());
    assert_eq!(PFR_KTABLE_CNT.get(), 0);
    assert_eq!(pf_main_ruleset().tables.get(), 0);
    assert_eq!(PFR_KTABLE_PL.pr_nout.get(), 0);
}

#[test]
fn anchored_tables_and_their_root() {
    let _g = setup();

    assert_eq!(add_tables(std::vec![table("t2", "/an", 0)]), 1);
    // The anchor was rewritten without its slash, and the root table made.
    let kt = pfr_lookup_table(&table("t2", "an", 0)).expect("anchored table");
    let root = pfr_lookup_table(&table("t2", "", 0)).expect("root table");
    assert!(ptr::eq(kt.pfrkt_root.get().unwrap(), root));
    assert_eq!(PFR_KTABLE_CNT.get(), 2);
    assert_eq!(root.pfrkt_refcnt()[PFR_REFCNT_ANCHOR].get(), 1);
    assert!(root.pfrkt_flags().get() & PFR_TFLAG_REFDANCHOR != 0);
    assert!(root.pfrkt_flags().get() & PFR_TFLAG_ACTIVE == 0);
    let an = pf_find_anchor(b"an").expect("the table's anchor");
    assert_eq!(an.ruleset.tables.get(), 1);
    assert_eq!(pfr_table_count(&table("", "an", 0), 0), 1);
    assert_eq!(pfr_table_count(&table("", "", 0), PFR_FLAG_ALLRSETS), 2);
    assert_eq!(pfr_table_count(&table("", "nowhere", 0), 0), -1);

    // An inactive anchored table falls back on an active root.
    let mut rt = table("t2", "", 0);
    assert_eq!(add_tables(std::vec![table("t2", "", 0)]), 0);
    assert!(
        root.pfrkt_flags().get() & PFR_TFLAG_ACTIVE != 0,
        "reactivated"
    );
    add_addrs(&mut rt, &[addr4([8, 8, 8, 8], 32, false)]).unwrap();
    assert!(pfr_match_addr(root, &pf4([8, 8, 8, 8]), AF_INET));
    kt.pfrkt_flags()
        .set(kt.pfrkt_flags().get() & !PFR_TFLAG_ACTIVE);
    assert!(ptr::eq(pfr_ktable_select_active(kt).unwrap(), root));
    assert!(pfr_kentry_byaddr(kt, &pf4([8, 8, 8, 8]), AF_INET, true).is_some());
    kt.pfrkt_flags()
        .set(kt.pfrkt_flags().get() | PFR_TFLAG_ACTIVE);

    // pfr_get_tables with an anchor filter.
    let mut out: Vec<PfrTable> = (0..2).map(|_| PfrTable::zeroed()).collect();
    let mut size = 2;
    pfr_get_tables(
        &mut table("", "an", 0),
        &mut PfrBuf::Kernel(&mut out),
        &mut size,
        0,
    )
    .unwrap();
    assert_eq!(size, 1);
    assert_eq!(pf_cstr(&out[0].pfrt_name), b"t2");
    assert_eq!(pf_cstr(&out[0].pfrt_anchor), b"an");

    // Deleting the anchored table drops the root's anchor reference: the root, active but
    // neither persistent nor referenced, goes too, and so does the emptied anchor.
    let mut ts = std::vec![table("t2", "an", 0)];
    pfr_del_tables(&PfrBuf::Kernel(&mut ts), 1, None, 0).unwrap();
    assert!(pfr_lookup_table(&table("t2", "an", 0)).is_none());
    assert!(pfr_lookup_table(&table("t2", "", 0)).is_none());
    assert!(pf_find_anchor(b"an").is_none());
    assert!(PF_ANCHORS.is_empty());
    assert_eq!(PFR_KTABLE_CNT.get(), 0);
    assert_eq!(PFR_KTABLE_PL.pr_nout.get(), 0);
}

#[test]
fn counters_follow_the_packets() {
    let _g = setup();
    add_tables(std::vec![table("c", "", PFR_TFLAG_COUNTERS)]);
    let mut tbl = table("c", "", 0);
    add_addrs(&mut tbl, &[addr4([10, 0, 0, 0], 8, false)]).unwrap();
    let kt = pfr_lookup_table(&tbl).unwrap();

    let mut pd = PfPdesc::new();
    pd.af = AF_INET;
    pd.tot_len = 100;
    pd.dir = PF_OUT;
    pfr_update_stats(kt, &pf4([10, 2, 3, 4]), &pd, PF_PASS, false);
    pfr_update_stats(kt, &pf4([10, 2, 3, 4]), &pd, PF_PASS, false);
    pd.dir = crate::net::pfvar::PF_IN;
    pfr_update_stats(kt, &pf4([10, 9, 9, 9]), &pd, PF_DROP, false);
    // Not in the table, though the rule said it was: counted as XPASS.
    pfr_update_stats(kt, &pf4([1, 1, 1, 1]), &pd, PF_PASS, false);

    assert_eq!(kt.pfrkt_packets()[1][PFR_OP_PASS].get(), 2);
    assert_eq!(kt.pfrkt_bytes()[1][PFR_OP_PASS].get(), 200);
    assert_eq!(kt.pfrkt_packets()[0][PFR_OP_BLOCK].get(), 1);
    assert_eq!(kt.pfrkt_packets()[0][PFR_OP_XPASS].get(), 1);

    let mut st = std::vec![PfrAstats::default(); 1];
    let mut size = 1;
    pfr_get_astats(&mut tbl, &mut PfrBuf::Kernel(&mut st), &mut size, 0).unwrap();
    assert_eq!(size, 1);
    assert_eq!(st[0].pfras_a.pfra_net, 8);
    assert_eq!(st[0].pfras_packets[1][PFR_OP_PASS], 2);
    assert_eq!(st[0].pfras_bytes[1][PFR_OP_PASS], 200);
    assert_eq!(st[0].pfras_packets[0][PFR_OP_BLOCK], 1);

    // pfr_clr_astats drops the entry's counters.
    let mut v = std::vec![addr4([10, 0, 0, 0], 8, false)];
    let mut nzero = -1;
    pfr_clr_astats(
        &mut tbl,
        &mut PfrBuf::Kernel(&mut v),
        1,
        Some(&mut nzero),
        0,
    )
    .unwrap();
    assert_eq!(nzero, 1);
    pfr_get_astats(&mut tbl, &mut PfrBuf::Kernel(&mut st), &mut size, 0).unwrap();
    assert_eq!(st[0].pfras_a.pfra_fback, PFR_FB_NOCOUNT);
    assert_eq!(PFR_KCOUNTERS_PL.pr_nout.get(), 0);

    // States of load balancing.
    assert_eq!(
        pfr_states_increase(kt, &pf4([10, 0, 0, 0]), AF_INET),
        -1,
        "not exact"
    );
    let mut v = std::vec![addr4([10, 0, 0, 7], 32, false)];
    pfr_add_addrs(&mut tbl, &mut PfrBuf::Kernel(&mut v), 1, None, 0).unwrap();
    assert_eq!(pfr_states_increase(kt, &pf4([10, 0, 0, 7]), AF_INET), 1);
    assert_eq!(pfr_states_increase(kt, &pf4([10, 0, 0, 7]), AF_INET), 2);
    assert_eq!(pfr_states_decrease(kt, &pf4([10, 0, 0, 7]), AF_INET), 1);

    // Table counters cleared.
    let mut ts = std::vec![table("c", "", 0)];
    let mut nzero = -1;
    pfr_clr_tstats(&PfrBuf::Kernel(&mut ts), 1, Some(&mut nzero), 0).unwrap();
    assert_eq!(nzero, 1);
    assert_eq!(kt.pfrkt_packets()[1][PFR_OP_PASS].get(), 0);
}

#[test]
fn transaction_define_and_commit() {
    let _g = setup();
    let trs = table("", "", 0);

    net_lock();
    pf_lock();
    let mut ticket = 0;
    pfr_ina_begin(&trs, Some(&mut ticket), None, 0).unwrap();
    assert!(pf_main_ruleset().topen.get() != 0);

    let mut tbl = table("def", "", 0);
    let a = PfrBuf::Kernel(&mut []);
    assert_eq!(
        pfr_ina_define(&mut tbl, &a, 0, None, None, ticket + 1, 0),
        Err(Errno::EBUSY)
    );
    let mut v = std::vec![addr4([172, 16, 0, 0], 12, false)];
    let (mut nadd, mut naddr) = (-1, -1);
    let mut tbl = table("def", "", PFR_TFLAG_PERSIST);
    pfr_ina_define(
        &mut tbl,
        &PfrBuf::Kernel(&mut v),
        1,
        Some(&mut nadd),
        Some(&mut naddr),
        ticket,
        PFR_FLAG_ADDRSTOO,
    )
    .unwrap();
    assert_eq!((nadd, naddr), (1, 1));
    let kt = pfr_lookup_table(&table("def", "", 0)).expect("inactive table");
    assert!(kt.pfrkt_flags().get() & PFR_TFLAG_INACTIVE != 0);
    assert!(
        !pfr_match_addr(kt, &pf4([172, 20, 1, 1]), AF_INET),
        "not active yet"
    );

    let (mut nadd, mut nchange) = (-1, -1);
    pfr_ina_commit(&trs, ticket, Some(&mut nadd), Some(&mut nchange), 0).unwrap();
    assert_eq!((nadd, nchange), (1, 0));
    assert!(kt.pfrkt_flags().get() & PFR_TFLAG_ACTIVE != 0);
    assert!(kt.pfrkt_flags().get() & PFR_TFLAG_INACTIVE == 0);
    assert!(kt.pfrkt_shadow.get().is_none());
    assert!(pfr_match_addr(kt, &pf4([172, 20, 1, 1]), AF_INET));
    assert_eq!(pf_main_ruleset().topen.get(), 0);

    // A second transaction merges into the active table.
    pfr_ina_begin(&trs, Some(&mut ticket), None, 0).unwrap();
    let mut v = std::vec![
        addr4([192, 0, 2, 0], 24, false),
        addr4([172, 16, 0, 0], 12, true)
    ];
    let mut tbl = table("def", "", PFR_TFLAG_PERSIST);
    pfr_ina_define(
        &mut tbl,
        &PfrBuf::Kernel(&mut v),
        2,
        Some(&mut nadd),
        Some(&mut naddr),
        ticket,
        PFR_FLAG_ADDRSTOO,
    )
    .unwrap();
    // The C counts an active table being redefined as added.
    assert_eq!((nadd, naddr), (1, 2));
    pfr_ina_commit(&trs, ticket, Some(&mut nadd), Some(&mut nchange), 0).unwrap();
    assert_eq!((nadd, nchange), (0, 1));
    assert!(kt.pfrkt_flags().get() & PFR_TFLAG_PERSIST != 0);
    assert!(pfr_match_addr(kt, &pf4([192, 0, 2, 5]), AF_INET));
    assert!(
        !pfr_match_addr(kt, &pf4([172, 20, 1, 1]), AF_INET),
        "now negated"
    );
    assert_eq!(kt.pfrkt_cnt().get(), 2);

    // A rolled back transaction leaves the table alone.
    pfr_ina_begin(&trs, Some(&mut ticket), None, 0).unwrap();
    let mut tbl = table("def", "", 0);
    pfr_ina_define(&mut tbl, &PfrBuf::Kernel(&mut []), 0, None, None, ticket, 0).unwrap();
    let mut ndel = -1;
    pfr_ina_rollback(&trs, ticket, Some(&mut ndel), 0).unwrap();
    assert_eq!(ndel, 1);
    assert!(kt.pfrkt_shadow.get().is_none());
    assert!(pfr_match_addr(kt, &pf4([192, 0, 2, 5]), AF_INET));
    pf_unlock();
    net_unlock();
}

#[test]
fn attach_and_detach_tables() {
    let _g = setup();

    pf_lock();
    let rs: &'static PfRuleset = pf_main_ruleset();
    let kt = pfr_attach_table(rs, b"rt", PR_WAITOK).expect("table");
    assert_eq!(kt.pfrkt_refcnt()[PFR_REFCNT_RULE].get(), 1);
    assert!(kt.pfrkt_flags().get() & PFR_TFLAG_REFERENCED != 0);
    assert!(ptr::eq(pfr_attach_table(rs, b"rt", PR_WAITOK).unwrap(), kt));
    pfr_detach_table(kt);
    assert!(pfr_lookup_table(&table("rt", "", 0)).is_some());
    pfr_detach_table(kt);
    assert!(
        pfr_lookup_table(&table("rt", "", 0)).is_none(),
        "unreferenced, destroyed"
    );
    assert_eq!(PFR_KTABLE_CNT.get(), 0);
    pf_unlock();

    // pfr_insert_kentry / pfr_remove_kentry, as pf's overload tables use them.
    add_tables(std::vec![table("ov", "", 0)]);
    let kt = pfr_lookup_table(&table("ov", "", 0)).unwrap();
    pfr_insert_kentry(kt, &addr4([203, 0, 113, 9], 32, false), 0).unwrap();
    pfr_insert_kentry(kt, &addr4([203, 0, 113, 9], 32, false), 0).unwrap();
    assert_eq!(kt.pfrkt_cnt().get(), 1);
    assert!(pfr_match_addr(kt, &pf4([203, 0, 113, 9]), AF_INET));
    pfr_remove_kentry(kt, &addr4([203, 0, 113, 9], 32, false)).unwrap();
    assert_eq!(
        pfr_remove_kentry(kt, &addr4([203, 0, 113, 9], 32, false)),
        Err(Errno::ESRCH)
    );
    assert_eq!(kt.pfrkt_cnt().get(), 0);
}

/// A kernel pool over the table `kt`.
fn rr_pool(kt: &'static PfrKtable) -> &'static PfPool {
    let p: &'static PfPool = Box::leak(Box::new(PfPool::zeroed()));
    p.addr.type_.set(PF_ADDR_TABLE);
    p.addr.set_tbl(Some(kt));
    p
}

#[test]
fn pool_get_round_robin() {
    let _g = setup();
    add_tables(std::vec![table("rr", "", 0)]);
    let mut tbl = table("rr", "", 0);
    add_addrs(
        &mut tbl,
        &[
            addr4([10, 0, 0, 1], 32, false),
            addr4([10, 0, 0, 2], 32, false),
            addr4([10, 0, 0, 8], 31, false),
            addr4([10, 0, 0, 5], 32, true),
        ],
    )
    .unwrap();
    let kt = pfr_lookup_table(&tbl).unwrap();
    let rpool = rr_pool(kt);

    let mut got = Vec::new();
    for _ in 0..5 {
        let (raddr, rmask) = pfr_pool_get(rpool, AF_INET).expect("an address");
        let c = rpool.counter.get();
        assert!(
            pf_match_addr(0, &raddr, &rmask, &c, AF_INET),
            "inside its block"
        );
        got.push(c.addr8[3]);
        // pf_lb steps the counter past the address it used.
        let mut c = c;
        pf_addr_inc(&mut c, AF_INET);
        rpool.counter.set(c);
    }
    // The negated 10.0.0.5 is skipped; the /31 gives both its addresses; then around.
    assert_eq!(got, [1, 2, 8, 9, 1]);
    assert_eq!(rpool.weight.get(), 1);

    // An empty table has nothing to give; a pool not over a table is -1.
    add_tables(std::vec![table("empty", "", 0)]);
    let empty = pfr_lookup_table(&table("empty", "", 0)).unwrap();
    assert_eq!(pfr_pool_get(rr_pool(empty), AF_INET), Err(1));
    let other: &'static PfPool = Box::leak(Box::new(PfPool::zeroed()));
    assert_eq!(pfr_pool_get(other, AF_INET), Err(-1));
}

#[test]
fn validate_and_fix() {
    let _g = setup();
    assert!(pfr_validate_addr(&addr4([10, 0, 0, 0], 8, false)));
    assert!(pfr_validate_addr(&addr4([0, 0, 0, 0], 0, false)));
    assert!(
        !pfr_validate_addr(&addr4([10, 0, 0, 1], 8, false)),
        "host bits"
    );
    assert!(
        !pfr_validate_addr(&addr4([10, 0, 0, 0], 33, false)),
        "prefix"
    );
    let mut a = addr4([10, 0, 0, 0], 8, false);
    a.pfra_not = 2;
    assert!(!pfr_validate_addr(&a));
    let mut a = addr4([10, 0, 0, 0], 8, false);
    a.pfra_af = 0;
    assert!(!pfr_validate_addr(&a));

    let mut anchor = [0u8; PATH_MAX];
    anchor[..6].copy_from_slice(b"///a/b");
    pfr_fix_anchor(&mut anchor).unwrap();
    assert_eq!(pf_cstr(&anchor), b"a/b");
    assert!(anchor[PATH_MAX - 3..].iter().all(|&b| b == 0));
    let mut anchor = [b'x'; PATH_MAX];
    assert_eq!(pfr_fix_anchor(&mut anchor), Err(Errno::ENAMETOOLONG));

    let mut t = table("", "", 0);
    assert_eq!(pfr_validate_table(&mut t, 0, false), Err(Errno::EINVAL));
    let mut t = table("x", "_pf", 0);
    assert_eq!(pfr_validate_table(&mut t, 0, true), Err(Errno::EINVAL));
    assert_eq!(pfr_validate_table(&mut t, 0, false), Ok(()));
    let mut t = table("x", "", PFR_TFLAG_PERSIST);
    assert_eq!(pfr_validate_table(&mut t, 0, false), Err(Errno::EINVAL));
    assert_eq!(pfr_validate_table(&mut t, PFR_TFLAG_USRMASK, false), Ok(()));

    assert_eq!(pfr_gcd(12, 18), 6);
    assert_eq!(pfr_gcd(0, 5), 5);
}
