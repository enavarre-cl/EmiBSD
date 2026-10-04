use super::*;
use crate::reftest::{assert_complete, assert_defines};
use crate::sys::endian::{htonl, htons, ntohl, ntohs};

#[test]
fn header_layout_on_the_wire() {
    let mut th = Tcphdr {
        th_sport: htons(12345),
        th_dport: htons(80),
        th_seq: htonl(0x0102_0304),
        th_ack: htonl(0x0a0b_0c0d),
        th_flags: TH_SYN | TH_ACK,
        th_win: htons(TCP_MAXWIN as u16),
        ..Tcphdr::default()
    };
    th.set_th_off((size_of::<Tcphdr>() >> 2) as u8);
    assert_eq!((th.th_off(), th.th_x2()), (5, 0));
    // SAFETY: `Tcphdr` is `repr(C)` integers without padding; its 20 bytes are initialized.
    let bytes: [u8; 20] = unsafe { core::mem::transmute(th) };
    assert_eq!(
        bytes,
        [
            0x30, 0x39, 0, 80, 1, 2, 3, 4, 0x0a, 0x0b, 0x0c, 0x0d, 0x50, 0x12, 0xff, 0xff, 0, 0, 0,
            0
        ]
    );
    assert_eq!((ntohs(th.th_sport), ntohl(th.th_ack)), (12345, 0x0a0b_0c0d));
}

#[test]
fn offset_and_x2_are_independent_nibbles() {
    let mut th = Tcphdr::default();
    th.set_th_x2(0xf);
    th.set_th_off(15);
    assert_eq!(th.th_x2_off, 0xff);
    th.set_th_off(6);
    assert_eq!((th.th_off(), th.th_x2(), th.th_x2_off), (6, 0xf, 0x6f));
    th.set_th_x2(0x12); // only the low four bits are kept
    assert_eq!((th.th_off(), th.th_x2()), (6, 2));
    th.set_th_reseqlen(1460);
    assert_eq!((th.th_reseqlen(), th.th_urp), (1460, 1460));
}

#[test]
fn option_words() {
    assert_eq!(TCPOPT_TSTAMP_HDR, 0x0101_080a);
    assert_eq!(TCPOPT_SACK_PERMIT_HDR, 0x0101_0402);
    assert_eq!(TCPOPT_SACK_HDR, 0x0101_0500);
    assert_eq!((TCPOLEN_TSTAMP_APPA, TCPOLEN_SIGLEN), (12, 20));
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/netinet/tcp.h");
    let th = assert_defines!(defs;
        TH_FIN, TH_SYN, TH_RST, TH_PUSH, TH_ACK, TH_URG, TH_ECE, TH_CWR);
    assert_complete(&defs, "TH_", &th);
    let mut opt = assert_defines!(defs;
        TCPOPT_EOL, TCPOPT_NOP, TCPOPT_MAXSEG, TCPOPT_WINDOW, TCPOPT_SACK_PERMITTED,
        TCPOPT_SACK, TCPOPT_TIMESTAMP, TCPOPT_SIGNATURE, TCPOPT_SACK_HDR);
    // `TCPOPT_TSTAMP_HDR` and `TCPOPT_SACK_PERMIT_HDR` continue on a second line, which the
    // define reader does not follow; `option_words` checks their values.
    assert_eq!(
        defs.get("TCPOPT_TSTAMP_HDR").map(|v| v.as_str()),
        Some("\\")
    );
    assert_eq!(
        defs.get("TCPOPT_SACK_PERMIT_HDR").map(|v| v.as_str()),
        Some("\\")
    );
    opt.extend(["TCPOPT_TSTAMP_HDR", "TCPOPT_SACK_PERMIT_HDR"]);
    assert_complete(&defs, "TCPOPT_", &opt);
    let len = assert_defines!(defs;
        TCPOLEN_MAXSEG, TCPOLEN_WINDOW, TCPOLEN_SACK_PERMITTED, TCPOLEN_SACK,
        TCPOLEN_TIMESTAMP, TCPOLEN_TSTAMP_APPA, TCPOLEN_SIGNATURE, TCPOLEN_SIGLEN);
    assert_complete(&defs, "TCPOLEN_", &len);
    let tcpi = assert_defines!(defs;
        TCPI_OPT_TIMESTAMPS, TCPI_OPT_SACK, TCPI_OPT_WSCALE, TCPI_OPT_ECN, TCPI_OPT_TOE);
    assert_complete(&defs, "TCPI_", &tcpi);
    let tcp = assert_defines!(defs;
        TCP_MAX_SACK, TCP_SACKHOLE_LIMIT, TCP_MSS, TCP_MAXWIN, TCP_MAX_WINSHIFT, TCP_NODELAY,
        TCP_MAXSEG, TCP_MD5SIG, TCP_SACK_ENABLE, TCP_INFO, TCP_NOPUSH);
    assert_complete(&defs, "TCP_", &tcp);
    assert_defines!(defs; MAX_TCPOPTLEN, MAX_SACK_BLKS);
}
