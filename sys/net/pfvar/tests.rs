use core::mem::{offset_of, size_of};

use super::*;
use crate::reftest::assert_defines;

/// The sizes and offsets clang computes for the C header (`-target x86_64-unknown-openbsd
/// -D_KERNEL`; arm64 gives the same, both are LP64 with natural alignment). pfctl(8) is
/// compiled against the C structures, so every ioctl argument must match them.
#[test]
fn layout() {
    assert_eq!(size_of::<PfRule>(), 1360);
    assert_eq!(size_of::<PfiocRule>(), 3424);
    assert_eq!(size_of::<PfStatus>(), 520);
    assert_eq!(size_of::<PfsyncState>(), 264);
    assert_eq!(size_of::<PfiKif>(), 256);
    assert_eq!(size_of::<PfSrcNode>(), 152);
    assert_eq!(size_of::<PfQueuespec>(), 320);
    assert_eq!(size_of::<PfrAddr>(), 52);
    assert_eq!(size_of::<PfrAstats>(), 160);
    assert_eq!(size_of::<PfrTstats>(), 1232);
    assert_eq!(size_of::<PfrTable>(), 1064);
    assert_eq!(size_of::<PfAddrWrap>(), 48);
    assert_eq!(size_of::<PfPool>(), 136);
    assert_eq!(size_of::<PfRuleAddr>(), 56);
    assert_eq!(size_of::<PfiocTable>(), 1104);
    assert_eq!(size_of::<PfiocIface>(), 40);
    assert_eq!(size_of::<PfiocTrans>(), 16);
    assert_eq!(size_of::<PfiocStates>(), 16);
    assert_eq!(size_of::<PfOsfpIoctl>(), 136);
    assert_eq!(size_of::<PfiocStatelim>(), 128);
    assert_eq!(size_of::<PfiocSourcelim>(), 208);
    assert_eq!(size_of::<PfiocSource>(), 64);
    assert_eq!(size_of::<PfiocSourceEntry>(), 56);
    assert_eq!(size_of::<PfiocStateKill>(), 224);
    assert_eq!(size_of::<PfiocNatlook>(), 80);
    assert_eq!(size_of::<PfiocRuleset>(), 1092);
    assert_eq!(size_of::<PfiocQueue>(), 328);
    assert_eq!(size_of::<PfiocQstats>(), 344);
    assert_eq!(size_of::<PfiocSrcNodeKill>(), 128);
    assert_eq!(size_of::<PfiocTm>(), 8);
    assert_eq!(size_of::<PfiocLimit>(), 8);
    assert_eq!(size_of::<PfiocSynflwats>(), 8);
    assert_eq!(size_of::<PfiocSrcNodes>(), 16);
    assert_eq!(size_of::<PfiocSourceKill>(), 48);
    assert_eq!(size_of::<PfiocTransE>(), 1032);
    assert_eq!(size_of::<PfOsfpEntry>(), 112);
    assert_eq!(size_of::<PfThreshold>(), 16);
    assert_eq!(size_of::<PfsyncStatePeer>(), 32);
    assert_eq!(size_of::<PfsyncStateKey>(), 40);
    assert_eq!(size_of::<PfRuleUid>(), 12);

    assert_eq!(offset_of!(PfRule, skip), 112);
    assert_eq!(offset_of!(PfRule, label), 184);
    assert_eq!(offset_of!(PfRule, entries), 568);
    assert_eq!(offset_of!(PfRule, nat), 584);
    assert_eq!(offset_of!(PfRule, rdr), 720);
    assert_eq!(offset_of!(PfRule, route), 856);
    assert_eq!(offset_of!(PfRule, pktrate), 992);
    assert_eq!(offset_of!(PfRule, evaluations), 1008);
    assert_eq!(offset_of!(PfRule, kif), 1048);
    assert_eq!(offset_of!(PfRule, os_fingerprint), 1080);
    assert_eq!(offset_of!(PfRule, rtableid), 1084);
    assert_eq!(offset_of!(PfRule, timeout), 1092);
    assert_eq!(offset_of!(PfRule, states_cur), 1172);
    assert_eq!(offset_of!(PfRule, max_src_conn_rate), 1200);
    assert_eq!(offset_of!(PfRule, cuid), 1228);
    assert_eq!(offset_of!(PfRule, return_icmp), 1236);
    assert_eq!(offset_of!(PfRule, uid), 1252);
    assert_eq!(offset_of!(PfRule, gid), 1264);
    assert_eq!(offset_of!(PfRule, rule_flag), 1276);
    assert_eq!(offset_of!(PfRule, action), 1280);
    assert_eq!(offset_of!(PfRule, af), 1288);
    assert_eq!(offset_of!(PfRule, type_), 1290);
    assert_eq!(offset_of!(PfRule, flags), 1294);
    assert_eq!(offset_of!(PfRule, flush), 1304);
    assert_eq!(offset_of!(PfRule, naf), 1308);
    assert_eq!(offset_of!(PfRule, rcvifnot), 1309);
    assert_eq!(offset_of!(PfRule, statelim), 1312);
    assert_eq!(offset_of!(PfRule, sourcelim), 1320);
    assert_eq!(offset_of!(PfRule, divert), 1328);
    assert_eq!(offset_of!(PfRule, exptime), 1352);

    assert_eq!(offset_of!(PfPool, key), 48);
    assert_eq!(offset_of!(PfPool, counter), 64);
    assert_eq!(offset_of!(PfPool, ifname), 80);
    assert_eq!(offset_of!(PfPool, kif), 96);
    assert_eq!(offset_of!(PfPool, tblidx), 104);
    assert_eq!(offset_of!(PfPool, states), 112);
    assert_eq!(offset_of!(PfPool, curweight), 120);
    assert_eq!(offset_of!(PfPool, weight), 124);
    assert_eq!(offset_of!(PfPool, opts), 131);

    assert_eq!(offset_of!(PfStatus, since), 440);
    assert_eq!(offset_of!(PfStatus, running), 448);
    assert_eq!(offset_of!(PfStatus, syncookies_active), 480);
    assert_eq!(offset_of!(PfStatus, ifname), 484);
    assert_eq!(offset_of!(PfStatus, pf_chksum), 500);

    assert_eq!(offset_of!(PfiKif, pfik_tree), 16);
    assert_eq!(offset_of!(PfiKif, pfik_packets), 48);
    assert_eq!(offset_of!(PfiKif, pfik_tzero), 176);
    assert_eq!(offset_of!(PfiKif, pfik_flags), 184);
    assert_eq!(offset_of!(PfiKif, pfik_ah_cookie), 192);
    assert_eq!(offset_of!(PfiKif, pfik_states), 216);
    assert_eq!(offset_of!(PfiKif, pfik_dynaddrs), 240);

    assert_eq!(offset_of!(PfSrcNode, addr), 32);
    assert_eq!(offset_of!(PfSrcNode, rule), 64);
    assert_eq!(offset_of!(PfSrcNode, kif), 72);
    assert_eq!(offset_of!(PfSrcNode, bytes), 80);
    assert_eq!(offset_of!(PfSrcNode, states), 112);
    assert_eq!(offset_of!(PfSrcNode, conn_rate), 120);
    assert_eq!(offset_of!(PfSrcNode, creation), 136);
    assert_eq!(offset_of!(PfSrcNode, af), 144);

    assert_eq!(offset_of!(PfsyncState, ifname), 8);
    assert_eq!(offset_of!(PfsyncState, key), 24);
    assert_eq!(offset_of!(PfsyncState, src), 104);
    assert_eq!(offset_of!(PfsyncState, rt_addr), 168);
    assert_eq!(offset_of!(PfsyncState, rule), 184);
    assert_eq!(offset_of!(PfsyncState, packets), 204);
    assert_eq!(offset_of!(PfsyncState, creatorid), 236);
    assert_eq!(offset_of!(PfsyncState, rtableid), 240);
    assert_eq!(offset_of!(PfsyncState, max_mss), 248);
    assert_eq!(offset_of!(PfsyncState, state_flags), 260);
    assert_eq!(offset_of!(PfsyncState, set_prio), 262);

    assert_eq!(offset_of!(PfQueuespec, qname), 16);
    assert_eq!(offset_of!(PfQueuespec, realtime), 160);
    assert_eq!(offset_of!(PfQueuespec, linkshare), 200);
    assert_eq!(offset_of!(PfQueuespec, flowqueue), 280);
    assert_eq!(offset_of!(PfQueuespec, kif), 296);
    assert_eq!(offset_of!(PfQueuespec, flags), 304);
    assert_eq!(offset_of!(PfQueuespec, qid), 312);

    assert_eq!(offset_of!(PfrTstats, pfrts_packets), 1064);
    assert_eq!(offset_of!(PfrTstats, pfrts_match), 1192);
    assert_eq!(offset_of!(PfrTstats, pfrts_tzero), 1208);
    assert_eq!(offset_of!(PfrTstats, pfrts_cnt), 1216);
    assert_eq!(offset_of!(PfrAstats, pfras_packets), 56);
    assert_eq!(offset_of!(PfrAstats, pfras_tzero), 152);

    assert_eq!(offset_of!(PfiocTable, pfrio_buffer), 1064);
    assert_eq!(offset_of!(PfiocTable, pfrio_esize), 1072);
    assert_eq!(offset_of!(PfiocTable, pfrio_ticket), 1100);
    assert_eq!(offset_of!(PfiocTrans, array), 8);
    assert_eq!(offset_of!(PfiocQstats, buf), 328);
    assert_eq!(offset_of!(PfiocQstats, nbytes), 336);
    assert_eq!(offset_of!(PfiocStatelim, limit), 24);
    assert_eq!(offset_of!(PfiocStatelim, description), 36);
    assert_eq!(offset_of!(PfiocStatelim, inuse), 100);
    assert_eq!(offset_of!(PfiocStatelim, admitted), 104);
    assert_eq!(offset_of!(PfiocSourcelim, overload_tblname), 40);
    assert_eq!(offset_of!(PfiocSourcelim, inet_prefix), 80);
    assert_eq!(offset_of!(PfiocSourcelim, description), 88);
    assert_eq!(offset_of!(PfiocSourcelim, nentries), 152);
    assert_eq!(offset_of!(PfiocSourcelim, addrallocs), 160);
    assert_eq!(offset_of!(PfiocSource, entry_size), 32);
    assert_eq!(offset_of!(PfiocSource, key), 40);
    assert_eq!(offset_of!(PfiocSourceEntry, addr), 8);
    assert_eq!(offset_of!(PfiocSourceEntry, inuse), 24);
    assert_eq!(offset_of!(PfiocSourceEntry, admitted), 32);
    assert_eq!(offset_of!(PfiocSourceKill, af), 24);
    assert_eq!(offset_of!(PfiocSourceKill, addr), 28);
    assert_eq!(offset_of!(PfiocSourceKill, rmstates), 44);
    assert_eq!(offset_of!(PfiocStateKill, psk_af), 16);
    assert_eq!(offset_of!(PfiocStateKill, psk_proto), 20);
    assert_eq!(offset_of!(PfiocStateKill, psk_src), 24);
    assert_eq!(offset_of!(PfiocStateKill, psk_ifname), 136);
    assert_eq!(offset_of!(PfiocStateKill, psk_killed), 216);
    assert_eq!(offset_of!(PfiocStateKill, psk_rdomain), 220);
    assert_eq!(offset_of!(PfiocSrcNodeKill, psnk_src), 8);
    assert_eq!(offset_of!(PfiocSrcNodeKill, psnk_killed), 120);
    assert_eq!(offset_of!(PfOsfpIoctl, fp_tcpopts), 112);
    assert_eq!(offset_of!(PfOsfpIoctl, fp_getnum), 132);
    assert_eq!(offset_of!(PfiocQueue, queue), 8);
    assert_eq!(offset_of!(PfiocIface, pfiio_buffer), 16);
    assert_eq!(offset_of!(PfiocRule, rule), 2064);
}

/// The ioctl numbers encode the structure sizes; a few spelled out from the C's `_IOWR`.
#[test]
fn ioctl_numbers() {
    use crate::sys::ioccom::{IOC_INOUT, IOC_VOID};
    let iowr = |n: u64, len: u64| IOC_INOUT | ((len & 0x1fff) << 16) | (u64::from(b'D') << 8) | n;
    assert_eq!(DIOCSTART, IOC_VOID | (u64::from(b'D') << 8) | 1);
    assert_eq!(DIOCSTOP, IOC_VOID | (u64::from(b'D') << 8) | 2);
    assert_eq!(DIOCADDRULE, iowr(4, 3424));
    assert_eq!(DIOCGETSTATUS, iowr(21, 520));
    assert_eq!(DIOCXBEGIN, iowr(81, 16));
    assert_eq!(DIOCRADDTABLES, iowr(61, 1104));
    assert_eq!(DIOCIGETIFACES, iowr(87, 40));
}

#[test]
fn addresses() {
    let mut a = PfAddr::zeroed();
    a.set_v4(crate::netinet::in_::InAddr {
        s_addr: 0x0a00_0202u32.to_be(),
    });
    assert_eq!(a.addr8[..4], [10, 0, 2, 2]);
    assert_eq!(u32::from_be(a.addr32(0)), 0x0a00_0202);
    assert_eq!(u16::from_be(a.addr16(1)), 0x0202);
    let b = a;
    assert!(pf_aeq(&a, &b, crate::sys::socket::AF_INET));
    assert!(!pf_aneq(&a, &b, crate::sys::socket::AF_INET));
    assert!(pf_azero(&PfAddr::zeroed(), crate::sys::socket::AF_INET));
    assert!(pf_pool_dyntype(PF_POOL_ROUNDROBIN | PF_POOL_STICKYADDR));
    assert!(!pf_pool_dyntype(PF_POOL_BITMASK));
    let packed = pf_osfp_pack(5, 6, 7);
    assert_eq!(pf_osfp_unpack(packed), (5, 6, 7));
    assert_eq!(PF_OSFP_MAX_OPTS, 21);
}

#[test]
fn abi_copies() {
    let mut data = [0u8; 3424];
    let mut pr: alloc::boxed::Box<PfiocRule> = pf_abi_zeroed();
    pr.ticket = 7;
    pr.rule.action = PF_DROP;
    pr.rule.nr.set(3);
    pf_abi_write(&mut data, &*pr);
    let back: alloc::boxed::Box<PfiocRule> = pf_abi_read(&data);
    assert_eq!(back.ticket, 7);
    assert_eq!(back.rule.action, PF_DROP);
    assert_eq!(back.rule.nr.get(), 3);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/net/pfvar.h");
    assert_defines!(defs;
        PF_MD5_DIGEST_LENGTH, PFTM_TCP_FIRST_PACKET_VAL, PFTM_TCP_OPENING_VAL,
        PFTM_TCP_FIN_WAIT_VAL, PFTM_TCP_CLOSED_VAL, PFTM_UDP_FIRST_PACKET_VAL,
        PFTM_UDP_SINGLE_VAL, PFTM_UDP_MULTIPLE_VAL, PFTM_ICMP_FIRST_PACKET_VAL,
        PFTM_ICMP_ERROR_REPLY_VAL, PFTM_OTHER_FIRST_PACKET_VAL, PFTM_OTHER_SINGLE_VAL,
        PFTM_OTHER_MULTIPLE_VAL, PFTM_FRAG_VAL, PFTM_INTERVAL_VAL, PFTM_SRC_NODE_VAL,
        PFTM_TS_DIFF_VAL, PF_FRAG_STALE, PF_FRAG_ENTRY_POINTS, PF_FRAG_ENTRY_LIMIT,
        PF_POOL_IDMASK, PF_POOL_TYPEMASK, PF_POOL_STICKYADDR, PF_WSCALE_FLAG, PF_WSCALE_MASK,
        PF_LOG, PF_LOG_ALL, PF_LOG_USER, PF_LOG_FORCE, PF_LOG_MATCHES, PF_TABLE_NAME_SIZE,
        PFI_AFLAG_NETWORK, PFI_AFLAG_BROADCAST, PFI_AFLAG_PEER, PFI_AFLAG_MODEMASK,
        PFI_AFLAG_NOALIAS, PF_THRESHOLD_MULT, PF_OSFP_EXPANDED, PF_OSFP_GENERIC,
        PF_OSFP_NODETAIL, PF_OSFP_LEN, PF_OSFP_WSIZE_MOD, PF_OSFP_WSIZE_DC, PF_OSFP_WSIZE_MSS,
        PF_OSFP_WSIZE_MTU, PF_OSFP_PSIZE_MOD, PF_OSFP_PSIZE_DC, PF_OSFP_WSCALE,
        PF_OSFP_WSCALE_MOD, PF_OSFP_WSCALE_DC, PF_OSFP_MSS, PF_OSFP_MSS_MOD, PF_OSFP_MSS_DC,
        PF_OSFP_DF, PF_OSFP_TS0, PF_OSFP_INET6, PF_OSFP_MAXTTL_OFFSET, PF_OSFP_TCPOPT_NOP,
        PF_OSFP_TCPOPT_WSCALE, PF_OSFP_TCPOPT_MSS, PF_OSFP_TCPOPT_SACK, PF_OSFP_TCPOPT_TS,
        PF_OSFP_TCPOPT_BITS, PF_ANCHOR_STACK_MAX, PF_ANCHOR_NAME_SIZE, PF_ANCHOR_HIWAT,
        PF_SKIP_IFP, PF_SKIP_DIR, PF_SKIP_RDOM, PF_SKIP_AF, PF_SKIP_PROTO, PF_SKIP_SRC_ADDR,
        PF_SKIP_DST_ADDR, PF_SKIP_SRC_PORT, PF_SKIP_DST_PORT, PF_SKIP_COUNT,
        PF_RULE_LABEL_SIZE, PF_QNAME_SIZE, PF_TAG_NAME_SIZE, PF_STATE_NORMAL,
        PF_STATE_MODULATE, PF_STATE_SYNPROXY, PF_FLUSH, PF_FLUSH_GLOBAL, PFRULE_DROP,
        PFRULE_RETURNRST, PFRULE_FRAGMENT, PFRULE_RETURNICMP, PFRULE_RETURN, PFRULE_NOSYNC,
        PFRULE_SRCTRACK, PFRULE_RULESRCTRACK, PFRULE_SETDELAY, PFRULE_IFBOUND,
        PFRULE_STATESLOPPY, PFRULE_PFLOW, PFRULE_ONCE, PFRULE_AFTO, PFRULE_EXPIRED,
        PFSTATE_HIWAT, PFSTATE_ADAPT_START, PFSTATE_ADAPT_END, PF_PKTDELAY_MAXPKTS,
        PFSNODE_HIWAT, PFSS_TIMESTAMP, PFSS_PAWS, PFSS_PAWS_IDLED, PFSS_DATA_TS,
        PFSS_DATA_NOTS, PFSTATE_ALLOWOPTS, PFSTATE_SLOPPY, PFSTATE_PFLOW, PFSTATE_NOSYNC,
        PFSTATE_ACK, PFSTATE_NODF, PFSTATE_SETTOS, PFSTATE_RANDOMID, PFSTATE_SCRUB_TCP,
        PFSTATE_SETPRIO, PFSTATE_INP_UNLINKED, PFSYNC_SCRUB_FLAG_VALID, PFSYNC_FLAG_SRCNODE,
        PFSYNC_FLAG_NATSRCNODE, PFR_TFLAG_PERSIST, PFR_TFLAG_CONST, PFR_TFLAG_ACTIVE,
        PFR_TFLAG_INACTIVE, PFR_TFLAG_REFERENCED, PFR_TFLAG_REFDANCHOR, PFR_TFLAG_COUNTERS,
        PFR_TFLAG_USRMASK, PFR_TFLAG_SETMASK, PFR_TFLAG_ALLMASK, PFRKE_FLAG_NOT,
        PFRKE_FLAG_MARK, PFI_IFLAG_SKIP, PFI_IFLAG_ANY, PF_DPORT_RANGE, PF_RPORT_RANGE,
        PFRES_MATCH, PFRES_BADOFF, PFRES_FRAG, PFRES_SHORT, PFRES_NORM, PFRES_MEMORY,
        PFRES_TS, PFRES_CONGEST, PFRES_IPOPTIONS, PFRES_PROTCKSUM, PFRES_BADSTATE,
        PFRES_STATEINS, PFRES_MAXSTATES, PFRES_SRCLIMIT, PFRES_SYNPROXY, PFRES_TRANSLATE,
        PFRES_NOROUTE, PFRES_MAX, LCNT_STATES, LCNT_SRCSTATES, LCNT_SRCNODES, LCNT_SRCCONN,
        LCNT_SRCCONNRATE, LCNT_OVERLOAD_TABLE, LCNT_OVERLOAD_FLUSH, LCNT_SYNFLOODS,
        LCNT_SYNCOOKIES_SENT, LCNT_SYNCOOKIES_VALID, LCNT_MAX, PFUDPS_NO_TRAFFIC,
        PFUDPS_SINGLE, PFUDPS_MULTIPLE, PFUDPS_NSTATES, PFOTHERS_NO_TRAFFIC, PFOTHERS_SINGLE,
        PFOTHERS_MULTIPLE, PFOTHERS_NSTATES, FCNT_STATE_SEARCH, FCNT_STATE_INSERT,
        FCNT_STATE_REMOVALS, FCNT_MAX, SCNT_SRC_NODE_SEARCH, SCNT_SRC_NODE_INSERT,
        SCNT_SRC_NODE_REMOVALS, SCNT_MAX, NCNT_FRAG_SEARCH, NCNT_FRAG_INSERT,
        NCNT_FRAG_REMOVALS, NCNT_MAX, PF_REASS_ENABLED, PF_REASS_NODF, PF_SYNCOOKIES_NEVER,
        PF_SYNCOOKIES_ALWAYS, PF_SYNCOOKIES_ADAPTIVE, PF_SYNCOOKIES_HIWATPCT, PF_PRIO_ZERO,
        PFQS_FLOWQUEUE, PFQS_ROOTCLASS, PFQS_DEFAULT, PFR_KTABLE_HIWAT, PFR_KENTRY_HIWAT,
        PFR_KENTRY_HIWAT_SMALL, PFR_FLAG_DUMMY, PFR_FLAG_FEEDBACK, PFR_FLAG_CLSTATS,
        PFR_FLAG_ADDRSTOO, PFR_FLAG_REPLACE, PFR_FLAG_ALLRSETS, PFR_FLAG_ALLMASK,
        PFR_FLAG_USERIOCTL, PF_STATELIM_NAME_LEN, PF_STATELIM_DESCR_LEN, PF_STATELIM_ID_NONE,
        PF_STATELIM_ID_MIN, PF_STATELIM_ID_MAX, PF_STATELIM_LIMIT_MIN, PF_SOURCELIM_NAME_LEN,
        PF_SOURCELIM_DESCR_LEN, PF_SOURCELIM_ID_NONE, PF_SOURCELIM_ID_MIN,
        PF_SOURCELIM_ID_MAX);
    // Spelled as expressions in the C.
    assert_eq!(PFTM_TCP_ESTABLISHED_VAL, 24 * 60 * 60);
    assert_eq!(PFTM_TCP_CLOSING_VAL, 15 * 60);
    assert_eq!(PF_STATELIM_LIMIT_MAX, 1 << 24);
}
