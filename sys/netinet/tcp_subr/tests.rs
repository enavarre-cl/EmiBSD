//! Host tests of `tcp_subr.rs`: window scaling, the header bytes the signature hashes, the
//! notify rules and a control block's life.

use std::boxed::Box;

use super::*;
use crate::netinet::tcp::TH_SYN;

fn test_tcpcb() -> &'static Tcpcb {
    let inp: &'static Inpcb = Box::leak(Box::new(Inpcb::new(None, None)));
    Box::leak(Box::new(Tcpcb::new(inp)))
}

#[test]
fn rscale_requests_the_smallest_sufficient_shift() {
    let tp = test_tcpcb();
    tcp_rscale(tp, 16 * 1024);
    assert_eq!(tp.request_r_scale.get(), 0);
    tcp_rscale(tp, 65536);
    assert_eq!(tp.request_r_scale.get(), 1);
    tcp_rscale(tp, 2 * 1024 * 1024);
    assert_eq!(tp.request_r_scale.get(), 6);
    tcp_rscale(tp, u64::MAX);
    assert_eq!(tp.request_r_scale.get(), TCP_MAX_WINSHIFT);
}

#[test]
fn header_bytes_follow_the_c_layout() {
    let mut th = Tcphdr {
        th_sport: htons(1234),
        th_dport: htons(80),
        th_seq: htonl(0x0102_0304),
        th_ack: htonl(0x0a0b_0c0d),
        th_flags: TH_SYN,
        th_win: htons(512),
        th_sum: 0xffff,
        th_urp: 0,
        ..Tcphdr::default()
    };
    th.set_th_off(5);
    let b = tcphdr_bytes(&th);
    assert_eq!(&b[0..4], &[0x04, 0xd2, 0x00, 0x50]);
    assert_eq!(&b[4..12], &[1, 2, 3, 4, 0x0a, 0x0b, 0x0c, 0x0d]);
    assert_eq!(b[12], 0x50);
    assert_eq!(b[13], TH_SYN);
    assert_eq!(&b[14..18], &[0x02, 0x00, 0xff, 0xff]);
}
