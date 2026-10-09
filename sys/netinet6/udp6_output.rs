/*	$KAME: udp6_output.c,v 1.21 2001/02/07 11:51:54 itojun Exp $	*/
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
 * Copyright (C) 1995, 1996, 1997, and 1998 WIDE Project.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the project nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE PROJECT AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE PROJECT OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */

/*
 * Copyright (c) 1982, 1986, 1989, 1993
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
 */
/* </LICENSES> */

/* <CODE> */
//! UDP output for IPv6 sockets: `netinet6/udp6_output.c`. UDP protocol implementation, per
//! RFC 768, August, 1980.
//!
//! Upstream: sys/netinet6/udp6_output.c @ 3ce1f3f79392
//!
//! `udp6_output` is `udp_output`'s IPv6 half (`netinet/udp_usrreq.rs` calls it for an
//! `INP_IPV6` control block): it checks the destination (an explicit `sockaddr_in6`, or the
//! connected peer), picks the source address, binds a local port if there is none yet,
//! prepends the IPv6 and UDP headers and hands the datagram to `ip6_output` with the
//! checksum left to `in6_proto_cksum_out` (`M_UDP_CSUM_OUT`).
//!
//! ## Deviations
//! - The destination `sockaddr_in6` is read out of `addr6` as a copy (the C copies it too,
//!   "protect *sin6 from overwrites"); mbuf data need not be aligned.
//! - The headers are written by `udp6_output_hdr`, a helper of this file (the C fills them
//!   inline), so that host tests can check them.
//! - `NPF` and `NSTOEPLITZ` are configured: `pf_mbuf_link_inpcb` and the flow id of a
//!   connected socket.

use core::mem::size_of;

use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{m_freem, m_prepend};
use crate::machine::cpu::curproc;
use crate::net::pf::pf_mbuf_link_inpcb;
use crate::netinet::in_::IPPROTO_UDP;
use crate::netinet::in_pcb::{IN6P_MINMTU, Inpcb, in_pcbbind};
use crate::netinet::ip6::{IPV6_FLOWINFO_MASK, IPV6_VERSION, IPV6_VERSION_MASK, Ip6Hdr};
use crate::netinet::udp::Udphdr;
use crate::netinet::udp_var::{UdpstatCounters, udpstat_inc};
use crate::netinet6::in6::{
    In6Addr, SockaddrIn6, in6_are_addr_equal, in6_is_addr_unspecified, in6_is_addr_v4mapped,
    in6_nam2sin6,
};
use crate::netinet6::in6_pcb::{in6_pcbaddrisavail, inp_moptions6, inp_outputopts6};
use crate::netinet6::in6_src::{in6_embedscope, in6_pcbselsrc, in6_selecthlim};
use crate::netinet6::ip6_output::{ip6_clearpktopts, ip6_output, ip6_setpktopts};
use crate::netinet6::ip6_var::{IPV6_MINMTU, Ip6Pktopts, mtod_ip6, mtod_ip6_store};
use crate::sys::endian::htons;
use crate::sys::errno::Errno;
use crate::sys::mbuf::{M_DONTWAIT, M_FLOWID, M_UDP_CSUM_OUT, Mbuf, mtod};
use crate::sys::proc::Proc;
use crate::sys::socketvar::{SS_ISCONNECTED, SS_PRIV};

/// `curproc`, which the socket requests run as.
fn curproc_or_panic(func: &str) -> &'static Proc {
    match curproc() {
        Some(p) => p,
        None => panic(format_args!("{}: no curproc", func)),
    }
}

/// `udp6_output`: sends datagram `m` of `inp` to the `sockaddr_in6` in `addr6` (`None`:
/// the connected peer) with control messages `control`. Consumes `m` and `control`.
pub fn udp6_output(
    inp: &'static Inpcb,
    m: &'static Mbuf,
    addr6: Option<&Mbuf>,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let ulen = m.m_pkthdr().len.get() as u32;
    let plen = size_of::<Udphdr>() as u32 + ulen;
    let mut opt = Ip6Pktopts::default();

    let priv_ = inp.socket().has_state(SS_PRIV);

    let result: Result<(), Errno> = 'releaseopt: {
        let error: Errno = 'release: {
            let optp: Option<&Ip6Pktopts> = if let Some(control) = control {
                if let Err(e) =
                    ip6_setpktopts(control, &mut opt, inp_outputopts6(inp), priv_, IPPROTO_UDP)
                {
                    break 'release e;
                }
                Some(&opt)
            } else {
                inp_outputopts6(inp)
            };

            let (laddr, faddr, fport): (In6Addr, In6Addr, u16) = if let Some(addr6) = addr6 {
                let sin6p = match in6_nam2sin6(addr6) {
                    Ok(p) => p,
                    Err(e) => break 'release e,
                };
                // protect *sin6 from overwrites: a copy.
                // SAFETY: `in6_nam2sin6` checked the mbuf holds a whole `sockaddr_in6`;
                // read unaligned.
                let mut sin6: SockaddrIn6 = unsafe { sin6p.read_unaligned() };
                if sin6.sin6_port == 0 {
                    break 'release Errno::EADDRNOTAVAIL;
                }
                if in6_is_addr_v4mapped(&sin6.sin6_addr) {
                    break 'release Errno::EADDRNOTAVAIL;
                }
                if !in6_is_addr_unspecified(&inp.inp_faddr6.get()) {
                    break 'release Errno::EISCONN;
                }

                let fport = sin6.sin6_port; // allow 0 port

                // KAME hack: embed scopeid
                let mut a = sin6.sin6_addr;
                if in6_embedscope(&mut a, &sin6, inp_outputopts6(inp), inp_moptions6(inp)).is_err()
                {
                    break 'release Errno::EINVAL;
                }
                sin6.sin6_addr = a;
                let faddr = sin6.sin6_addr;

                let mut laddr = In6Addr::default();
                if let Err(e) = in6_pcbselsrc(&mut laddr, &sin6, inp, optp) {
                    break 'release e;
                }

                if inp.inp_lport.get() == 0
                    && let Err(e) = in_pcbbind(inp, None, curproc_or_panic("udp6_output"))
                {
                    break 'release e;
                }

                if !in6_is_addr_unspecified(&inp.inp_laddr6.get())
                    && !in6_are_addr_equal(&inp.inp_laddr6.get(), &laddr)
                {
                    let mut valid = SockaddrIn6::with_addr(laddr);
                    valid.sin6_port = inp.inp_lport.get();
                    valid.sin6_scope_id = 0;
                    if let Err(e) =
                        in6_pcbaddrisavail(inp, &mut valid, 0, curproc_or_panic("udp6_output"))
                    {
                        break 'release e;
                    }
                }
                (laddr, faddr, fport)
            } else {
                if in6_is_addr_unspecified(&inp.inp_faddr6.get()) {
                    break 'release Errno::ENOTCONN;
                }
                (
                    inp.inp_laddr6.get(),
                    inp.inp_faddr6.get(),
                    inp.inp_fport.get(),
                )
            };

            let hlen = size_of::<Ip6Hdr>();

            // Calculate data length and get a mbuf for UDP and IP6 headers.
            let Some(m) = m_prepend(m, (hlen + size_of::<Udphdr>()) as i32, M_DONTWAIT) else {
                break 'releaseopt Err(Errno::ENOBUFS);
            };

            // Stuff checksum and output datagram.
            udp6_output_hdr(m, inp, &laddr, &faddr, fport, plen);

            let ph = m.m_pkthdr();
            ph.csum_flags.set(ph.csum_flags.get() | M_UDP_CSUM_OUT);

            let mut flags = 0;
            if inp.has_flags(IN6P_MINMTU) {
                flags |= IPV6_MINMTU;
            }

            udpstat_inc(UdpstatCounters::UdpsOpackets);

            // force routing table
            ph.ph_rtableid.set(inp.inp_rtableid.get());

            if inp.socket().has_state(SS_ISCONNECTED) {
                pf_mbuf_link_inpcb(m, Some(inp));
                ph.ph_flowid.set(inp.inp_flowid.get());
                ph.csum_flags.set(ph.csum_flags.get() | M_FLOWID);
            }

            break 'releaseopt ip6_output(
                m,
                optp,
                Some(&inp.inp_route),
                flags,
                inp_moptions6(inp),
                Some(&inp.inp_seclevel.get()),
            );
        };

        // release:
        m_freem(m);
        Err(error)
    };

    // releaseopt:
    if let Some(control) = control {
        ip6_clearpktopts(&mut opt, -1);
        m_freem(control);
    }
    result
}

/// The IPv6 and UDP headers `udp6_output` writes at the front of `m` (prepended for them):
/// from `laddr` and `inp`'s local port to `faddr`.`fport`, `plen` bytes of UDP (header
/// included; a length that does not fit in 16 bits is written as 0), the checksum zero for
/// `in6_proto_cksum_out` to fill. `ip6_plen` is left to `ip6_output`.
fn udp6_output_hdr(m: &Mbuf, inp: &Inpcb, laddr: &In6Addr, faddr: &In6Addr, fport: u16, plen: u32) {
    let hlen = size_of::<Ip6Hdr>();

    let udp6 = Udphdr {
        uh_sport: inp.inp_lport.get(), // lport is always set in the PCB
        uh_dport: fport,
        uh_ulen: if plen <= 0xffff {
            htons(plen as u16)
        } else {
            0
        },
        uh_sum: 0,
    };
    // SAFETY: `m_prepend` made the first mbuf hold both headers' bytes; written unaligned.
    unsafe {
        mtod::<u8>(m)
            .add(hlen)
            .cast::<Udphdr>()
            .write_unaligned(udp6)
    };

    let mut ip6 = mtod_ip6(m);
    ip6.ip6_flow = inp.inp_flowinfo() & IPV6_FLOWINFO_MASK;
    ip6.set_ip6_vfc((ip6.ip6_vfc() & !IPV6_VERSION_MASK) | IPV6_VERSION);
    // ip6_plen will be filled in ip6_output.
    ip6.ip6_nxt = IPPROTO_UDP as u8;
    ip6.ip6_hlim = in6_selecthlim(inp) as u8;
    ip6.ip6_src = *laddr;
    ip6.ip6_dst = *faddr;
    mtod_ip6_store(m, &ip6);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for UDP output over IPv6: the headers `udp6_output` writes and the checksum
    // `in6_proto_cksum_out` then fills in for them (`M_UDP_CSUM_OUT`), and the destination
    // checks that fail before anything is sent.

    use std::boxed::Box;
    use std::{assert_eq, assert_ne};

    use super::*;
    use crate::kern::uipc_mbuf::m_get;
    use crate::kern::uipc_socket::soalloc;
    use crate::kern::uipc_socket2::{solock, sounlock};
    use crate::net::if_::tests::test_packet;
    use crate::netinet::in_pcb::tests::{setup, teardown};
    use crate::netinet::in_pcb::{Inpcbtable, in_pcballoc, in_pcbdetach, in_pcbinit};
    use crate::netinet::ip_input::tests::bytes;
    use crate::netinet6::in6::IN6ADDR_ANY;
    use crate::netinet6::in6::tests::a6;
    use crate::netinet6::in6_cksum::in6_cksum;
    use crate::netinet6::in6_pcb::tests::pcb_of;
    use crate::netinet6::in6_proto::INET6SW;
    use crate::netinet6::ip6_output::in6_proto_cksum_out;
    use crate::sys::endian::htonl;
    use crate::sys::errno::Errno;
    use crate::sys::mbuf::{M_WAIT, MT_SONAME};
    use crate::sys::socket::SOCK_DGRAM;
    use crate::sys::socketvar::{SS_NOFDREF, soref};

    /// An `INP_IPV6` control block of a datagram socket bound to `fd00:77::1` port 5353.
    fn pcb6() -> &'static Inpcb {
        let t: &'static Inpcbtable = Box::leak(Box::new(Inpcbtable::new()));
        in_pcbinit(t, 1);
        let so = soalloc(&INET6SW[1], M_WAIT).expect("socket");
        so.so_type.set(SOCK_DGRAM);
        in_pcballoc(so, t, M_WAIT).expect("in_pcballoc");
        let inp = pcb_of(so);
        inp.set_flags(crate::netinet::in_pcb::INP_IPV6);
        inp.inp_laddr6.set(a6("fd00:77::1"));
        inp.inp_lport.set(htons(5353));
        inp
    }

    /// Detaches the control block and lets the socket go.
    fn release(inp: &'static Inpcb) {
        let so = inp.socket();
        let _ = soref(Some(so));
        solock(so);
        so.set_state(SS_NOFDREF);
        in_pcbdetach(inp);
        sounlock(so);
        crate::kern::uipc_socket::sorele(so);
    }

    /// An address mbuf holding `sin6`.
    fn nam6(sin6: SockaddrIn6) -> &'static Mbuf {
        let m = m_get(M_DONTWAIT, MT_SONAME).expect("mbuf");
        m.m_len().set(size_of::<SockaddrIn6>() as u32);
        // SAFETY: a fresh mbuf of `MLEN` bytes.
        unsafe { mtod::<SockaddrIn6>(m).write_unaligned(sin6) };
        m
    }

    #[test]
    fn the_headers_and_the_checksum_of_a_datagram() {
        let (_g, _t, _p) = setup();
        let inp = pcb6();
        // A traffic class and a flow label; only the low 28 bits go to the header.
        inp.set_inp_flowinfo(htonl(0xfe_0a_bc_de));

        let payload = b"hello";
        let plen = (size_of::<Udphdr>() + payload.len()) as u32;
        let m = m_prepend(test_packet(payload), 48, M_DONTWAIT).expect("prepend");
        udp6_output_hdr(
            m,
            inp,
            &a6("fd00:77::1"),
            &a6("fd00:77::2"),
            htons(53),
            plen,
        );
        // ip6_output fills in the payload length.
        let mut ip6 = mtod_ip6(m);
        ip6.ip6_plen = htons(plen as u16);
        mtod_ip6_store(m, &ip6);

        let b = bytes(m);
        assert_eq!(b.len(), 40 + 8 + 5);
        assert_eq!(&b[..4], &[0x6e, 0x0a, 0xbc, 0xde], "version 6, flow info");
        assert_eq!(b[6], IPPROTO_UDP as u8);
        assert_eq!(b[7], 64, "the default hop limit");
        assert_eq!(&b[8..24], &a6("fd00:77::1").s6_addr);
        assert_eq!(&b[24..40], &a6("fd00:77::2").s6_addr);
        assert_eq!(&b[40..46], &[0x14, 0xe9, 0, 53, 0, 13], "ports and length");
        assert_eq!(&b[46..48], &[0, 0], "checksum left to in6_proto_cksum_out");
        assert_eq!(&b[48..], payload);

        // The checksum in software (no interface offloads it).
        let ph = m.m_pkthdr();
        ph.csum_flags.set(ph.csum_flags.get() | M_UDP_CSUM_OUT);
        in6_proto_cksum_out(m, None);
        let b = bytes(m);
        assert_ne!(&b[46..48], &[0, 0]);
        assert_eq!(in6_cksum(m, IPPROTO_UDP as u8, 40, plen), 0);
        m_freem(m);

        // A datagram longer than 64k has a zero UDP length (a jumbogram's).
        let m = m_prepend(test_packet(&[0]), 48, M_DONTWAIT).expect("prepend");
        udp6_output_hdr(
            m,
            inp,
            &a6("fd00:77::1"),
            &a6("fd00:77::2"),
            htons(53),
            70_000,
        );
        assert_eq!(&bytes(m)[44..46], &[0, 0]);
        m_freem(m);

        release(inp);
        teardown();
    }

    #[test]
    fn destinations_that_cannot_be_sent_to() {
        let (_g, _t, _p) = setup();
        let inp = pcb6();
        let send = |addr: Option<SockaddrIn6>| {
            let nam = addr.map(nam6);
            let r = udp6_output(inp, test_packet(&[1, 2, 3]), nam, None);
            m_freem(nam);
            r
        };

        // Unconnected without an address; a zero port; an IPv4-mapped address.
        assert_eq!(send(None), Err(Errno::ENOTCONN));
        assert_eq!(
            send(Some(SockaddrIn6::with_addr(a6("fd00:77::2")))),
            Err(Errno::EADDRNOTAVAIL)
        );
        let mapped = SockaddrIn6 {
            sin6_port: htons(53),
            ..SockaddrIn6::with_addr(a6("::ffff:a00:202"))
        };
        assert_eq!(send(Some(mapped)), Err(Errno::EADDRNOTAVAIL));
        // Not a sockaddr_in6.
        let mut short = SockaddrIn6::with_addr(IN6ADDR_ANY);
        short.sin6_family = 2;
        assert_eq!(send(Some(short)), Err(Errno::EAFNOSUPPORT));

        // Connected: no other destination.
        inp.inp_faddr6.set(a6("fd00:77::2"));
        let other = SockaddrIn6 {
            sin6_port: htons(53),
            ..SockaddrIn6::with_addr(a6("fd00:77::3"))
        };
        assert_eq!(send(Some(other)), Err(Errno::EISCONN));

        inp.inp_faddr6.set(IN6ADDR_ANY);
        release(inp);
        teardown();
    }
}
/* </TESTS> */
