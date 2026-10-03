use super::*;
use crate::reftest::{assert_complete, assert_defines};

#[test]
fn cmsg_macros() {
    assert_eq!(cmsg_len(4), 16 + 4);
    assert_eq!(cmsg_space(4), 16 + 8);
    assert_eq!(cmsg_space(0), 16);

    // Two control messages of 4 data bytes each in a 48-byte buffer.
    let mut buf = [0u64; 6];
    let control = buf.as_mut_ptr().cast::<c_void>();
    let mhdr = Msghdr {
        msg_name: core::ptr::null_mut(),
        msg_namelen: 0,
        msg_iov: core::ptr::null_mut(),
        msg_iovlen: 0,
        msg_control: control,
        msg_controllen: (2 * cmsg_space(4)) as Socklen,
        msg_flags: 0,
    };
    let first = cmsg_firsthdr(&mhdr);
    assert_eq!(first.cast::<c_void>(), control);
    assert_eq!(cmsg_data(first) as usize - first as usize, 16);
    // SAFETY: `first` points into `buf`, which is aligned for a `Cmsghdr`.
    unsafe { (*first).cmsg_len = cmsg_len(4) as Socklen };
    // SAFETY: as above.
    let second = unsafe { cmsg_nxthdr(&mhdr, first) };
    assert_eq!(second as usize - first as usize, cmsg_space(4));
    // SAFETY: `second` is the second header inside `buf`.
    unsafe { (*second).cmsg_len = cmsg_len(4) as Socklen };
    // SAFETY: as above.
    assert!(unsafe { cmsg_nxthdr(&mhdr, second) }.is_null());

    let short = Msghdr {
        msg_controllen: 4,
        ..mhdr
    };
    assert!(cmsg_firsthdr(&short).is_null());
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/sys/socket.h");
    let sock = assert_defines!(defs;
        SOCK_STREAM, SOCK_DGRAM, SOCK_RAW, SOCK_RDM, SOCK_SEQPACKET, SOCK_TYPE_MASK,
        SOCK_CLOEXEC, SOCK_NONBLOCK, SOCK_NONBLOCK_INHERIT, SOCK_DNS, SOCK_CLOFORK);
    assert_complete(&defs, "SOCK_", &sock);
    let so = assert_defines!(defs;
        SO_DEBUG, SO_ACCEPTCONN, SO_REUSEADDR, SO_KEEPALIVE, SO_DONTROUTE, SO_BROADCAST,
        SO_USELOOPBACK, SO_LINGER, SO_OOBINLINE, SO_REUSEPORT, SO_TIMESTAMP, SO_BINDANY,
        SO_ZEROIZE, SO_SNDBUF, SO_RCVBUF, SO_SNDLOWAT, SO_RCVLOWAT, SO_SNDTIMEO, SO_RCVTIMEO,
        SO_ERROR, SO_TYPE, SO_RTABLE, SO_PEERCRED, SO_SPLICE, SO_DOMAIN, SO_PROTOCOL,
        SOL_SOCKET, SOMAXCONN);
    assert_complete(&defs, "SO_", &so);
    let rt = assert_defines!(defs; RT_TABLEID_MAX, RT_TABLEID_BITS, RT_TABLEID_MASK);
    assert_complete(&defs, "RT_", &rt);
    let af = assert_defines!(defs;
        AF_UNSPEC, AF_UNIX, AF_LOCAL, AF_INET, AF_IMPLINK, AF_PUP, AF_CHAOS, AF_NS, AF_ISO,
        AF_OSI, AF_ECMA, AF_DATAKIT, AF_CCITT, AF_SNA, AF_DECnet, AF_DLI, AF_LAT, AF_HYLINK,
        AF_APPLETALK, AF_ROUTE, AF_LINK, AF_COIP, AF_CNT, AF_IPX, AF_INET6, AF_ISDN, AF_E164,
        AF_NATM, AF_ENCAP, AF_SIP, AF_KEY, AF_BLUETOOTH, AF_MPLS, AF_FRAME, AF_MAX);
    assert_complete(&defs, "AF_", &af);
    let pseudo = assert_defines!(defs;
        pseudo_AF_XTP, pseudo_AF_RTIP, pseudo_AF_PIP, pseudo_AF_HDRCMPLT, pseudo_AF_PFLOW,
        pseudo_AF_PIPEX);
    assert_complete(&defs, "pseudo_AF_", &pseudo);
    let pf = assert_defines!(defs;
        PF_UNSPEC, PF_LOCAL, PF_UNIX, PF_INET, PF_IMPLINK, PF_PUP, PF_CHAOS, PF_NS, PF_ISO,
        PF_OSI, PF_ECMA, PF_DATAKIT, PF_CCITT, PF_SNA, PF_DECnet, PF_DLI, PF_LAT, PF_HYLINK,
        PF_APPLETALK, PF_ROUTE, PF_LINK, PF_XTP, PF_COIP, PF_CNT, PF_IPX, PF_INET6, PF_RTIP,
        PF_PIP, PF_ISDN, PF_NATM, PF_ENCAP, PF_SIP, PF_KEY, PF_BPF, PF_BLUETOOTH, PF_MPLS,
        PF_PFLOW, PF_PIPEX, PF_FRAME, PF_MAX);
    assert_complete(&defs, "PF_", &pf);
    let shut = assert_defines!(defs; SHUT_RD, SHUT_WR, SHUT_RDWR);
    assert_complete(&defs, "SHUT_", &shut);
    let net = assert_defines!(defs;
        NET_MAXID, NET_RT_DUMP, NET_RT_FLAGS, NET_RT_IFLIST, NET_RT_STATS, NET_RT_TABLE,
        NET_RT_IFNAMES, NET_RT_SOURCE, NET_RT_MAXID, NET_UNIX_INFLIGHT, NET_UNIX_DEFERRED,
        NET_UNIX_MAXID, NET_UNIX_PROTO_MAXID, NET_LINK_IFRXQ, NET_LINK_MAXID,
        NET_LINK_IFRXQ_MAXID, NET_KEY_SADB_DUMP, NET_KEY_SPD_DUMP, NET_KEY_MAXID,
        NET_BPF_BUFSIZE, NET_BPF_MAXBUFSIZE, NET_BPF_MAXID, NET_PFLOW_STATS, NET_PFLOW_MAXID);
    // The two ifrxq pressure values are continued on the next line in the C.
    assert_eq!(NET_LINK_IFRXQ_PRESSURE_RETURN, 1);
    assert_eq!(NET_LINK_IFRXQ_PRESSURE_DROP, 2);
    let net = [
        &net[..],
        &[
            "NET_LINK_IFRXQ_PRESSURE_RETURN",
            "NET_LINK_IFRXQ_PRESSURE_DROP",
        ],
    ]
    .concat();
    assert_complete(&defs, "NET_", &net);
    let unp = assert_defines!(defs; UNPCTL_RECVSPACE, UNPCTL_SENDSPACE);
    assert_complete(&defs, "UNPCTL_", &unp);
    let msg = assert_defines!(defs;
        MSG_OOB, MSG_PEEK, MSG_DONTROUTE, MSG_EOR, MSG_TRUNC, MSG_CTRUNC, MSG_WAITALL,
        MSG_DONTWAIT, MSG_BCAST, MSG_MCAST, MSG_NOSIGNAL, MSG_CMSG_CLOEXEC, MSG_WAITFORONE,
        MSG_CMSG_CLOFORK);
    assert_complete(&defs, "MSG_", &msg);
    let scm = assert_defines!(defs; SCM_RIGHTS, SCM_TIMESTAMP);
    assert_complete(&defs, "SCM_", &scm);
}
