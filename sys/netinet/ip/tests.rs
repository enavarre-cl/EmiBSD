use super::*;
use crate::reftest::{assert_complete, assert_defines};
use crate::sys::endian::{htonl, htons, ntohs};

#[test]
fn header_layout_on_the_wire() {
    let mut ip = Ip::default();
    ip.set_ip_v(IPVERSION);
    ip.set_ip_hl((size_of::<Ip>() >> 2) as u8);
    ip.ip_len = htons(84);
    ip.ip_off = htons(IP_DF);
    ip.ip_ttl = IPDEFTTL;
    ip.ip_p = 1;
    ip.ip_src.s_addr = htonl(0x0a00_020f);
    ip.ip_dst.s_addr = htonl(0x0a00_0202);
    assert_eq!((ip.ip_v(), ip.ip_hl()), (4, 5));
    // SAFETY: `Ip` is `repr(C)` integers without padding; its 20 bytes are initialized.
    let bytes: [u8; 20] = unsafe { core::mem::transmute(ip) };
    assert_eq!(
        bytes,
        [
            0x45, 0, 0, 84, 0, 0, 0x40, 0, 64, 1, 0, 0, 10, 0, 2, 15, 10, 0, 2, 2
        ]
    );
    assert_eq!(ntohs(ip.ip_off) & IP_DF, IP_DF);

    let mut ts = IpTimestamp {
        ipt_code: IPOPT_TS,
        ipt_len: 12,
        ipt_ptr: 5,
        ipt_oflwflg: 0,
        ipt_timestamp: IptTimestamp { ipt_time: [0] },
    };
    ts.set_ipt_flg(IPOPT_TS_PRESPEC);
    ts.set_ipt_oflw(2);
    assert_eq!(ts.ipt_oflwflg, 0x23);
    assert_eq!((ts.ipt_flg(), ts.ipt_oflw()), (3, 2));
    assert_eq!(ipopt_number(IPOPT_LSRR), 3);
    assert_eq!(ipopt_copied(IPOPT_LSRR), 0x80);
    assert_eq!(ipopt_class(IPOPT_TS), IPOPT_DEBMEAS);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/netinet/ip.h");
    let ip = assert_defines!(defs; IP_RF, IP_DF, IP_MF, IP_OFFMASK, IP_MAXPACKET, IP_MSS);
    assert_complete(&defs, "IP_", &ip);
    let tos = assert_defines!(defs;
        IPTOS_LOWDELAY, IPTOS_THROUGHPUT, IPTOS_RELIABILITY, IPTOS_CE, IPTOS_ECT,
        IPTOS_PREC_NETCONTROL, IPTOS_PREC_INTERNETCONTROL, IPTOS_PREC_CRITIC_ECP,
        IPTOS_PREC_FLASHOVERRIDE, IPTOS_PREC_FLASH, IPTOS_PREC_IMMEDIATE, IPTOS_PREC_PRIORITY,
        IPTOS_PREC_ROUTINE, IPTOS_DSCP_CS0, IPTOS_DSCP_LE, IPTOS_DSCP_CS1, IPTOS_DSCP_AF11,
        IPTOS_DSCP_AF12, IPTOS_DSCP_AF13, IPTOS_DSCP_CS2, IPTOS_DSCP_AF21, IPTOS_DSCP_AF22,
        IPTOS_DSCP_AF23, IPTOS_DSCP_CS3, IPTOS_DSCP_AF31, IPTOS_DSCP_AF32, IPTOS_DSCP_AF33,
        IPTOS_DSCP_CS4, IPTOS_DSCP_AF41, IPTOS_DSCP_AF42, IPTOS_DSCP_AF43, IPTOS_DSCP_CS5,
        IPTOS_DSCP_VA, IPTOS_DSCP_EF, IPTOS_DSCP_CS6, IPTOS_DSCP_CS7, IPTOS_ECN_NOTECT,
        IPTOS_ECN_ECT1, IPTOS_ECN_ECT0, IPTOS_ECN_CE, IPTOS_ECN_MASK);
    assert_complete(&defs, "IPTOS_", &tos);
    let opt = assert_defines!(defs;
        IPOPT_CONTROL, IPOPT_RESERVED1, IPOPT_DEBMEAS, IPOPT_RESERVED2, IPOPT_EOL, IPOPT_NOP,
        IPOPT_RR, IPOPT_TS, IPOPT_SECURITY, IPOPT_LSRR, IPOPT_SATID, IPOPT_SSRR, IPOPT_RA,
        IPOPT_OPTVAL, IPOPT_OLEN, IPOPT_OFFSET, IPOPT_MINOFF, IPOPT_TS_TSONLY,
        IPOPT_TS_TSANDADDR, IPOPT_TS_PRESPEC, IPOPT_SECUR_UNCLASS, IPOPT_SECUR_CONFID,
        IPOPT_SECUR_EFTO, IPOPT_SECUR_MMMM, IPOPT_SECUR_RESTR, IPOPT_SECUR_SECRET,
        IPOPT_SECUR_TOPSECRET);
    assert_complete(&defs, "IPOPT_", &opt);
    assert_defines!(defs; IPVERSION, MAXTTL, IPDEFTTL, IPFRAGTTL, IPTTLDEC, IPQ_MAXLEN);
}
