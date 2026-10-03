use super::*;
use crate::reftest::{assert_complete, assert_defines};
use crate::sys::endian::{htons, ntohs};

#[test]
fn echo_request_layout() {
    let mut icp = Icmp {
        icmp_type: ICMP_ECHO,
        icmp_code: 0,
        icmp_cksum: 0,
        icmp_hun: IcmpHun { ih_void: 0 },
        icmp_dun: IcmpDun {
            id_ip: IdIp::default(),
        },
    };
    icp.set_icmp_id(htons(0x1234));
    icp.set_icmp_seq(htons(7));
    assert_eq!(ntohs(icp.icmp_id()), 0x1234);
    assert_eq!(ntohs(icp.icmp_seq()), 7);
    // SAFETY: the first 8 bytes of the `repr(C)` header (type, code, checksum and `icmp_hun`,
    // built from the 4-byte `ih_void`) are initialized integers.
    let head: [u8; 8] = unsafe { *(&icp as *const Icmp).cast::<[u8; 8]>() };
    assert_eq!(head, [8, 0, 0, 0, 0x12, 0x34, 0, 7]);
    assert!(icmp_infotype(ICMP_ECHO) && icmp_infotype(ICMP_ECHOREPLY));
    assert!(!icmp_infotype(ICMP_UNREACH));

    icp.icmp_ip_mut().set_ip_hl(5);
    assert_eq!(icp.icmp_ip().ip_hl(), 5);
    assert_eq!(icmp_advlen(&icp), ICMP_ADVLENMIN);
    assert_eq!(icmp_v6advlen(&icp), ICMP_V6ADVLENMIN);
    let data = icp.icmp_data();
    assert_eq!(data as usize - &icp as *const Icmp as usize, ICMP_MINLEN);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/netinet/ip_icmp.h");
    let icmp = assert_defines!(defs;
        ICMP_EXT_HDR_VERSION, ICMP_EXT_HDR_VMASK, ICMP_EXT_OFFSET, ICMP_EXT_MPLS,
        ICMP_EXT_IFINFO, ICMP_MINLEN, ICMP_MASKLEN, ICMP_ADVLENMAX, ICMP_ECHOREPLY,
        ICMP_UNREACH, ICMP_UNREACH_NET, ICMP_UNREACH_HOST, ICMP_UNREACH_PROTOCOL,
        ICMP_UNREACH_PORT, ICMP_UNREACH_NEEDFRAG, ICMP_UNREACH_SRCFAIL,
        ICMP_UNREACH_NET_UNKNOWN, ICMP_UNREACH_HOST_UNKNOWN, ICMP_UNREACH_ISOLATED,
        ICMP_UNREACH_NET_PROHIB, ICMP_UNREACH_HOST_PROHIB, ICMP_UNREACH_TOSNET,
        ICMP_UNREACH_TOSHOST, ICMP_UNREACH_FILTER_PROHIB, ICMP_UNREACH_HOST_PRECEDENCE,
        ICMP_UNREACH_PRECEDENCE_CUTOFF, ICMP_SOURCEQUENCH, ICMP_REDIRECT, ICMP_REDIRECT_NET,
        ICMP_REDIRECT_HOST, ICMP_REDIRECT_TOSNET, ICMP_REDIRECT_TOSHOST, ICMP_ALTHOSTADDR,
        ICMP_ECHO, ICMP_ROUTERADVERT, ICMP_ROUTERADVERT_NORMAL,
        ICMP_ROUTERADVERT_NOROUTE_COMMON, ICMP_ROUTERSOLICIT, ICMP_TIMXCEED,
        ICMP_TIMXCEED_INTRANS, ICMP_TIMXCEED_REASS, ICMP_PARAMPROB, ICMP_PARAMPROB_ERRATPTR,
        ICMP_PARAMPROB_OPTABSENT, ICMP_PARAMPROB_LENGTH, ICMP_TSTAMP, ICMP_TSTAMPREPLY,
        ICMP_IREQ, ICMP_IREQREPLY, ICMP_MASKREQ, ICMP_MASKREPLY, ICMP_TRACEROUTE,
        ICMP_DATACONVERR, ICMP_MOBILE_REDIRECT, ICMP_IPV6_WHEREAREYOU, ICMP_IPV6_IAMHERE,
        ICMP_MOBILE_REGREQUEST, ICMP_MOBILE_REGREPLY, ICMP_SKIP, ICMP_PHOTURIS,
        ICMP_PHOTURIS_UNKNOWN_INDEX, ICMP_PHOTURIS_AUTH_FAILED, ICMP_PHOTURIS_DECRYPT_FAILED,
        ICMP_MAXTYPE);
    // The lengths written with sizeof: compare the text and the values.
    assert_eq!(defs["ICMP_TSLEN"], "(8 + 3 * sizeof (u_int32_t))");
    assert_eq!(ICMP_TSLEN, 20);
    assert_eq!(defs["ICMP_ADVLENMIN"], "(8 + sizeof (struct ip) + 8)");
    assert_eq!(ICMP_ADVLENMIN, 36);
    assert_eq!(defs["ICMP_V6ADVLENMIN"], "(8 + sizeof(struct ip) + 40)");
    assert_eq!(ICMP_V6ADVLENMIN, 68);
    let sized = ["ICMP_TSLEN", "ICMP_ADVLENMIN", "ICMP_V6ADVLENMIN"];
    assert_complete(&defs, "ICMP_", &[&icmp[..], &sized].concat());
}
