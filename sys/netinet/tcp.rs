/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

/*
 * Copyright (c) 1982, 1986, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)tcp.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! TCP header, per RFC 793, September 1981: `<netinet/tcp.h>`.
//!
//! Upstream: sys/netinet/tcp.h @ 3ce1f3f79392
//!
//! [`Tcphdr`] is the header as it is on the wire: multi-byte fields hold network-order values
//! (`ntohs(th.th_sport)`, `ntohl(th.th_seq)`), as in C. The constants are host-order values
//! (`TH_SYN` is compared with `th.th_flags & TH_SYN`, `TCPOPT_TSTAMP_HDR` is stored with
//! `htonl`).
//!
//! Status: `ported`.
//!
//! ## Deviations
//! - The C bit-fields `th_x2:4, th_off:4` (a `u_int32_t` bit-field of eight bits) are one
//!   byte, `th_x2_off`, with accessor methods (`th_off()`, `set_th_off()`, `th_x2()`,
//!   `set_th_x2()`). The C declares the fields in opposite orders for little- and big-endian
//!   machines so that the data offset is always the high nibble of byte 12 on the wire; the
//!   byte with explicit shifts is that layout on every machine.
//! - `#define th_reseqlen th_urp` (the member reused as the data length during reassembly) is
//!   the accessor pair `th_reseqlen()` / `set_th_reseqlen()` over `th_urp`.
//! - `typedef u_int32_t tcp_seq` is `pub type TcpSeq = u32` (the scalar typedef row of
//!   `docs/C_TO_RUST.md`: CamelCase alias of the same width).
//! - `struct tcp_info` keeps the C's members, including the `__tcpi_*` placeholders the kernel
//!   leaves at zero; it is the `TCP_INFO` socket option's ABI, so it is `#[repr(C)]`.

use core::mem::size_of;

/// `tcp_seq`: a TCP sequence number, compared modulo 2^32 (`netinet/tcp_seq.rs`).
pub type TcpSeq = u32;

// Flags of `th_flags`.

/// `TH_FIN`: no more data from the sender.
pub const TH_FIN: u8 = 0x01;
/// `TH_SYN`: synchronize sequence numbers.
pub const TH_SYN: u8 = 0x02;
/// `TH_RST`: reset the connection.
pub const TH_RST: u8 = 0x04;
/// `TH_PUSH`: push function.
pub const TH_PUSH: u8 = 0x08;
/// `TH_ACK`: the acknowledgement field is significant.
pub const TH_ACK: u8 = 0x10;
/// `TH_URG`: the urgent pointer field is significant.
pub const TH_URG: u8 = 0x20;
/// `TH_ECE`: ECN echo (RFC 3168).
pub const TH_ECE: u8 = 0x40;
/// `TH_CWR`: congestion window reduced (RFC 3168).
pub const TH_CWR: u8 = 0x80;

// Options: kinds (`TCPOPT_*`) and lengths (`TCPOLEN_*`).

/// End of option list.
pub const TCPOPT_EOL: u8 = 0;
/// No operation.
pub const TCPOPT_NOP: u8 = 1;
/// Maximum segment size.
pub const TCPOPT_MAXSEG: u8 = 2;
/// Length of the maximum segment size option.
pub const TCPOLEN_MAXSEG: u8 = 4;
/// Window scale.
pub const TCPOPT_WINDOW: u8 = 3;
/// Length of the window scale option.
pub const TCPOLEN_WINDOW: u8 = 3;
/// SACK permitted (experimental).
pub const TCPOPT_SACK_PERMITTED: u8 = 4;
/// Length of the SACK permitted option.
pub const TCPOLEN_SACK_PERMITTED: u8 = 2;
/// SACK (experimental).
pub const TCPOPT_SACK: u8 = 5;
/// Length of one SACK block: `2 * sizeof(tcp_seq)`.
pub const TCPOLEN_SACK: u8 = 8;
/// Timestamp.
pub const TCPOPT_TIMESTAMP: u8 = 8;
/// Length of the timestamp option.
pub const TCPOLEN_TIMESTAMP: u8 = 10;
/// Length of the timestamp option with its two NOPs (RFC 1323, appendix A).
pub const TCPOLEN_TSTAMP_APPA: u8 = TCPOLEN_TIMESTAMP + 2;
/// TCP MD5 signature (RFC 2385).
pub const TCPOPT_SIGNATURE: u8 = 19;
/// Length of the signature option.
pub const TCPOLEN_SIGNATURE: u8 = 18;
/// Length of the signature option with its padding.
pub const TCPOLEN_SIGLEN: u8 = TCPOLEN_SIGNATURE + 2;

/// Absolute maximum TCP options length.
pub const MAX_TCPOPTLEN: usize = 40;

/// The first word of an appendix-A timestamp option (NOP, NOP, kind, length), host order.
pub const TCPOPT_TSTAMP_HDR: u32 = (TCPOPT_NOP as u32) << 24
    | (TCPOPT_NOP as u32) << 16
    | (TCPOPT_TIMESTAMP as u32) << 8
    | TCPOLEN_TIMESTAMP as u32;

/// The SACK permitted option padded to a word (NOP, NOP, kind, length), host order.
pub const TCPOPT_SACK_PERMIT_HDR: u32 = (TCPOPT_NOP as u32) << 24
    | (TCPOPT_NOP as u32) << 16
    | (TCPOPT_SACK_PERMITTED as u32) << 8
    | TCPOLEN_SACK_PERMITTED as u32;

/// The first word of a SACK option (NOP, NOP, kind; the length byte is or-ed in), host order.
pub const TCPOPT_SACK_HDR: u32 =
    (TCPOPT_NOP as u32) << 24 | (TCPOPT_NOP as u32) << 16 | (TCPOPT_SACK as u32) << 8;

/// Max # SACK blocks stored at sender side.
pub const MAX_SACK_BLKS: usize = 6;
/// Max # SACKs sent in any segment.
pub const TCP_MAX_SACK: usize = 3;
/// Max # SACK holes per connection.
pub const TCP_SACKHOLE_LIMIT: i32 = 128;

/// Default maximum segment size for TCP. With an IP MSS of 576 this is 536, but 512 is probably
/// more convenient. This should be defined as `min(512, IP_MSS - sizeof (struct tcpiphdr))`.
pub const TCP_MSS: i32 = 512;

/// Largest value for the (unscaled) window.
pub const TCP_MAXWIN: u32 = 65535;

/// Maximum window shift.
pub const TCP_MAX_WINSHIFT: u8 = 14;

// `tcpi_options` bits of `struct tcp_info`.

/// Timestamps are in use.
pub const TCPI_OPT_TIMESTAMPS: u8 = 0x01;
/// SACK is in use.
pub const TCPI_OPT_SACK: u8 = 0x02;
/// Window scaling is in use.
pub const TCPI_OPT_WSCALE: u8 = 0x04;
/// ECN is in use.
pub const TCPI_OPT_ECN: u8 = 0x08;
/// TCP offload engine.
pub const TCPI_OPT_TOE: u8 = 0x10;

// User-settable options (used with setsockopt).

/// Don't delay send to coalesce packets.
pub const TCP_NODELAY: i32 = 0x01;
/// Set maximum segment size.
pub const TCP_MAXSEG: i32 = 0x02;
/// Enable TCP MD5 signature option.
pub const TCP_MD5SIG: i32 = 0x04;
/// Enable SACKs (if disabled by default).
pub const TCP_SACK_ENABLE: i32 = 0x08;
/// Retrieve the `tcp_info` structure.
pub const TCP_INFO: i32 = 0x09;
/// Don't push the last block of a write.
pub const TCP_NOPUSH: i32 = 0x10;

/// `struct tcphdr`: TCP header, per RFC 793, September 1981.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tcphdr {
    /// Source port, network order.
    pub th_sport: u16,
    /// Destination port, network order.
    pub th_dport: u16,
    /// Sequence number, network order.
    pub th_seq: TcpSeq,
    /// Acknowledgement number, network order.
    pub th_ack: TcpSeq,
    /// `th_off:4` (the high nibble, data offset in 32-bit words) and `th_x2:4` (the low
    /// nibble, unused).
    pub th_x2_off: u8,
    /// Flags (`TH_*`).
    pub th_flags: u8,
    /// Window, network order.
    pub th_win: u16,
    /// Checksum.
    pub th_sum: u16,
    /// Urgent pointer, network order; also `th_reseqlen`.
    pub th_urp: u16,
}

impl Tcphdr {
    /// `th_off`: data offset (header length), in 32-bit words.
    pub const fn th_off(&self) -> u8 {
        self.th_x2_off >> 4
    }

    /// Sets `th_off` (the low four bits of `off`).
    pub fn set_th_off(&mut self, off: u8) {
        self.th_x2_off = (self.th_x2_off & 0x0f) | (off << 4);
    }

    /// `th_x2`: the four unused bits.
    pub const fn th_x2(&self) -> u8 {
        self.th_x2_off & 0x0f
    }

    /// Sets `th_x2` (the low four bits of `x2`).
    pub fn set_th_x2(&mut self, x2: u8) {
        self.th_x2_off = (self.th_x2_off & 0xf0) | (x2 & 0x0f);
    }

    /// `th_reseqlen`: the TCP data length for resequencing/reassembly, kept in `th_urp`.
    pub const fn th_reseqlen(&self) -> u16 {
        self.th_urp
    }

    /// Sets `th_reseqlen` (`th_urp`).
    pub fn set_th_reseqlen(&mut self, len: u16) {
        self.th_urp = len;
    }
}

/// `struct tcp_info`: the `TCP_INFO` socket option, from the Linux 2.6 TCP API: an overlapping
/// set of fields with the Linux implementation plus OpenBSD specific information. Members from
/// `tcpi_snd_wnd` on are only set if the process is privileged, otherwise they are 0.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TcpInfo {
    /// TCP FSM state (`TCPS_*`).
    pub tcpi_state: u8,
    /// Linux compatibility, unused.
    pub __tcpi_ca_state: u8,
    /// Linux compatibility, unused.
    pub __tcpi_retransmits: u8,
    /// Linux compatibility, unused.
    pub __tcpi_probes: u8,
    /// Linux compatibility, unused.
    pub __tcpi_backoff: u8,
    /// Options enabled on the connection (`TCPI_OPT_*`).
    pub tcpi_options: u8,
    /// RFC 1323 send shift value.
    pub tcpi_snd_wscale: u8,
    /// RFC 1323 receive shift value.
    pub tcpi_rcv_wscale: u8,
    /// Retransmission timeout (usec).
    pub tcpi_rto: u32,
    /// Linux compatibility, unused.
    pub __tcpi_ato: u32,
    /// Max segment size for send.
    pub tcpi_snd_mss: u32,
    /// Max segment size for receive.
    pub tcpi_rcv_mss: u32,
    /// Linux compatibility, unused.
    pub __tcpi_unacked: u32,
    /// Linux compatibility, unused.
    pub __tcpi_sacked: u32,
    /// Linux compatibility, unused.
    pub __tcpi_lost: u32,
    /// Linux compatibility, unused.
    pub __tcpi_retrans: u32,
    /// Linux compatibility, unused.
    pub __tcpi_fackets: u32,
    /// Usecs since last sent data.
    pub tcpi_last_data_sent: u32,
    /// Usecs since last sent ack.
    pub tcpi_last_ack_sent: u32,
    /// Usecs since last received data.
    pub tcpi_last_data_recv: u32,
    /// Usecs since last received ack.
    pub tcpi_last_ack_recv: u32,
    /// Linux compatibility, unused.
    pub __tcpi_pmtu: u32,
    /// Linux compatibility, unused.
    pub __tcpi_rcv_ssthresh: u32,
    /// Smoothed RTT in usecs.
    pub tcpi_rtt: u32,
    /// RTT variance in usecs.
    pub tcpi_rttvar: u32,
    /// Slow start threshold.
    pub tcpi_snd_ssthresh: u32,
    /// Send congestion window.
    pub tcpi_snd_cwnd: u32,
    /// Linux compatibility, unused.
    pub __tcpi_advmss: u32,
    /// Linux compatibility, unused.
    pub __tcpi_reordering: u32,
    /// Linux compatibility, unused.
    pub __tcpi_rcv_rtt: u32,
    /// Advertised receive window.
    pub tcpi_rcv_space: u32,
    /// Advertised send window (FreeBSD/NetBSD extension).
    pub tcpi_snd_wnd: u32,
    /// Next egress seqno.
    pub tcpi_snd_nxt: u32,
    /// Next ingress seqno.
    pub tcpi_rcv_nxt: u32,
    /// HWTID for TOE endpoints.
    pub tcpi_toe_tid: u32,
    /// Retransmitted packets.
    pub tcpi_snd_rexmitpack: u32,
    /// Out-of-order packets.
    pub tcpi_rcv_ooopack: u32,
    /// Zero-sized windows sent.
    pub tcpi_snd_zerowin: u32,
    /// `t_rttmin` (OpenBSD extension).
    pub tcpi_rttmin: u32,
    /// `max_sndwnd`.
    pub tcpi_max_sndwnd: u32,
    /// `rcv_adv`.
    pub tcpi_rcv_adv: u32,
    /// `rcv_up`.
    pub tcpi_rcv_up: u32,
    /// `snd_una`.
    pub tcpi_snd_una: u32,
    /// `snd_up`.
    pub tcpi_snd_up: u32,
    /// `snd_wl1`.
    pub tcpi_snd_wl1: u32,
    /// `snd_wl2`.
    pub tcpi_snd_wl2: u32,
    /// `snd_max`.
    pub tcpi_snd_max: u32,
    /// `ts_recent`.
    pub tcpi_ts_recent: u32,
    /// `ts_recent_age`.
    pub tcpi_ts_recent_age: u32,
    /// `rfbuf_cnt`.
    pub tcpi_rfbuf_cnt: u32,
    /// `rfbuf_ts`.
    pub tcpi_rfbuf_ts: u32,
    /// `so_rcv.sb_cc`.
    pub tcpi_so_rcv_sb_cc: u32,
    /// `so_rcv.sb_hiwat`.
    pub tcpi_so_rcv_sb_hiwat: u32,
    /// `so_rcv.sb_lowat`.
    pub tcpi_so_rcv_sb_lowat: u32,
    /// `so_rcv.sb_wat`.
    pub tcpi_so_rcv_sb_wat: u32,
    /// `so_snd.sb_cc`.
    pub tcpi_so_snd_sb_cc: u32,
    /// `so_snd.sb_hiwat`.
    pub tcpi_so_snd_sb_hiwat: u32,
    /// `so_snd.sb_lowat`.
    pub tcpi_so_snd_sb_lowat: u32,
    /// `so_snd.sb_wat`.
    pub tcpi_so_snd_sb_wat: u32,
}

// Sizes of the C structures.
const _: () = {
    assert!(size_of::<Tcphdr>() == 20);
    assert!(core::mem::offset_of!(Tcphdr, th_x2_off) == 12);
    assert!(core::mem::offset_of!(Tcphdr, th_flags) == 13);
    assert!(size_of::<TcpInfo>() == 8 + 51 * 4);
    assert!(TCPOLEN_SACK as usize == 2 * size_of::<TcpSeq>());
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
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
                0x30, 0x39, 0, 80, 1, 2, 3, 4, 0x0a, 0x0b, 0x0c, 0x0d, 0x50, 0x12, 0xff, 0xff, 0,
                0, 0, 0
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
}
/* </TESTS> */
