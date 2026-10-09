/*	$KAME: mld6.c,v 1.26 2001/02/16 14:50:35 itojun Exp $	*/
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

/*-
 * Copyright (c) 2009 Bruce Simpson.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. The name of the author may not be used to endorse or promote
 *    products derived from this software without specific prior written
 *    permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 */

/*
 * Copyright (C) 1998 WIDE Project.
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
 * Copyright (c) 1988 Stephen Deering.
 * Copyright (c) 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Stephen Deering of Stanford University.
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
 *	@(#)igmp.c	8.1 (Berkeley) 7/19/93
 */
/* </LICENSES> */

/* <CODE> */
//! Multicast Listener Discovery: the MLDv2 message formats of `<netinet6/mld6.h>`, and
//! `netinet6/mld6.c`, the MLDv1 host side (joining and leaving groups, answering queries).
//!
//! Upstream: sys/netinet6/mld6.h @ 3ce1f3f79392
//! Upstream: sys/netinet6/mld6.c @ 3ce1f3f79392
//!
//! `mld6.c` shares the module with the header. See `<netinet/icmp6.h>`
//! (`netinet/icmp6.rs`) for `struct mld_hdr` (MLDv1 query and host report format).
//!
//! ## Deviations
//! - The `MLD_MRC_*`, `MLD_QQIC_*`, `MLD_QRESV`, `MLD_SFLAG` and `MLD_QRV` macros are
//!   `const fn`s; `mld_numrecs` is [`Mldv2Report::mld_numrecs`]. The structures are
//!   `#[repr(C)]` without padding (the C's are `__packed`; sizes asserted below).
//! - `ip6_opts` (`static struct ip6_pktopts`, with `ip6po_hbh` pointing at `hbh_buf`) is built
//!   by `mld6_ip6_opts` on each send from the static buffer [`MLD6_HBH_BUF`] that
//!   `mld6_init` fills: `Ip6Pktopts` holds `NonNull`s, so it cannot sit in a `static`.
//! - The lists of reports (`struct mld6_pktlist`, `STAILQ` of malloc'd entries) are
//!   `Vec<Mld6Pktinfo>`s, so queuing a report cannot fail (the C skips the group on `ENOMEM`).
//! - `mld6_input` clears and sets `mld_addr`'s embedded scope in a copy of the header, not in
//!   the packet (which it frees at the end anyway).
//! - `MROUTING` (`ip6_mrouter_active`, loopback of reports to the routing daemon) is not
//!   configured.

use alloc::vec::Vec;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use libkern::StaticCell;

use crate::kern::kern_rwlock::{
    rw_assert_anylock, rw_assert_wrlock, rw_enter_write, rw_exit_write,
};
use crate::kern::uipc_mbuf::{m_align, m_free, m_freem, m_get, m_gethdr};
use crate::net::if_::{IFF_LOOPBACK, IFNETLIST, if_get, if_put};
use crate::net::if_var::Ifnet;
use crate::netinet::icmp6::{
    Icmp6Hdr, Icmp6statCounters, MLD_LISTENER_DONE, MLD_LISTENER_QUERY, MLD_LISTENER_REPORT,
    MldHdr, icmp6stat_inc, icmp6stat_inc_hist,
};
use crate::netinet::in_::IPPROTO_ICMPV6;
use crate::netinet::ip6::{
    IP6OPT_PADN, IP6OPT_ROUTER_ALERT, IP6OPT_RTALERT_LEN, IP6OPT_RTALERT_MLD, IPV6_VERSION, Ip6Hbh,
    Ip6Hdr, ip6_exthdr_get,
};
use crate::netinet6::in6::{
    __IPV6_ADDR_SCOPE_INTFACELOCAL, __IPV6_ADDR_SCOPE_LINKLOCAL, __ipv6_addr_mc_scope, IN6ADDR_ANY,
    IN6ADDR_LINKLOCAL_ALLNODES, IN6ADDR_LINKLOCAL_ALLROUTERS, In6Addr, in6_are_addr_equal,
    in6_is_addr_linklocal, in6_is_addr_mc_linklocal, in6_is_addr_multicast,
    in6_is_addr_unspecified, in6_lookupmulti, in6ifa_ifpforlinklocal,
};
use crate::netinet6::in6_var::{
    IN6_IFF_ANYCAST, IN6_IFF_DUPLICATED, IN6_IFF_TENTATIVE, In6Multi, ia6_in6, ifmatoin6m,
};
use crate::netinet6::ip6_output::{ip6_initpktopts, ip6_output};
use crate::netinet6::ip6_var::{IPV6_UNSPECSRC, Ip6Moptions, Ip6Pktopts, mtod_ip6, mtod_ip6_store};
use crate::netinet6::mld6_var::{
    MLD_IREPORTEDLAST, MLD_OTHERLISTENER, Mld6Pktinfo, mld_random_delay,
};
use crate::sys::endian::{htons, ntohs};
use crate::sys::mbuf::{
    M_DONTWAIT, M_ICMP_CSUM_OUT, M_LOOP, MHLEN, MT_DATA, MT_HEADER, Mbuf, mtod,
};
use crate::sys::protosw::PR_FASTHZ;
use crate::sys::queue::ListHead;
use crate::sys::socket::AF_INET6;
use crate::sys::systm::{net_lock_shared, net_unlock_shared};

/// Minimum length of any MLD protocol message.
pub const MLD_MINLEN: usize = size_of::<Icmp6Hdr>();

/// `MLD_V2_REPORT_MAXRECS`.
pub const MLD_V2_REPORT_MAXRECS: u32 = 65535;

// MLDv2 report modes.

/// Don't send a record.
pub const MLD_DO_NOTHING: u8 = 0;
/// MODE_IN.
pub const MLD_MODE_IS_INCLUDE: u8 = 1;
/// MODE_EX.
pub const MLD_MODE_IS_EXCLUDE: u8 = 2;
/// TO_IN.
pub const MLD_CHANGE_TO_INCLUDE_MODE: u8 = 3;
/// TO_EX.
pub const MLD_CHANGE_TO_EXCLUDE_MODE: u8 = 4;
/// ALLOW_NEW.
pub const MLD_ALLOW_NEW_SOURCES: u8 = 5;
/// BLOCK_OLD.
pub const MLD_BLOCK_OLD_SOURCES: u8 = 6;

// MLDv2 query types.

/// `MLD_V2_GENERAL_QUERY`.
pub const MLD_V2_GENERAL_QUERY: i32 = 1;
/// `MLD_V2_GROUP_QUERY`.
pub const MLD_V2_GROUP_QUERY: i32 = 2;
/// `MLD_V2_GROUP_SOURCE_QUERY`.
pub const MLD_V2_GROUP_SOURCE_QUERY: i32 = 3;

/// Maximum report interval for MLDv1 host membership reports (in seconds).
pub const MLD_V1_MAX_RI: u32 = 10;

/// `MLD_TIMER_SCALE`: the MLD code field specifies time in milliseconds.
pub const MLD_TIMER_SCALE: u32 = 1000;

/// `struct mldv2_query`: MLD v2 query format (followed by 1..numsrc source addresses).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Mldv2Query {
    /// ICMPv6 header.
    pub mld_icmp6_hdr: Icmp6Hdr,
    /// Address being queried.
    pub mld_addr: In6Addr,
    /// Reserved/suppress/robustness.
    pub mld_misc: u8,
    /// Querier's query interval.
    pub mld_qqi: u8,
    /// Number of sources.
    pub mld_numsrc: u16,
}

/// `MLD_V2_QUERY_MINLEN`.
pub const MLD_V2_QUERY_MINLEN: usize = size_of::<Mldv2Query>();

/// `struct mldv2_report`: MLDv2 host membership report header (`mld_type`:
/// `MLDV2_LISTENER_REPORT`), followed by 1..numgrps records.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Mldv2Report {
    /// `mld_icmp6_hdr`.
    pub mld_icmp6_hdr: Icmp6Hdr,
}

impl Mldv2Report {
    /// `mld_numrecs` (`mld_icmp6_hdr.icmp6_data16[1]`), overlaid on the ICMPv6 header.
    pub fn mld_numrecs(&self) -> u16 {
        self.mld_icmp6_hdr.icmp6_data16(1)
    }

    /// `mld_numrecs = v`.
    pub fn set_mld_numrecs(&mut self, v: u16) {
        self.mld_icmp6_hdr.set_icmp6_data16(1, v);
    }
}

/// `struct mldv2_record`: a report record (followed by 1..numsrc source addresses).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mldv2Record {
    /// Record type.
    pub mr_type: u8,
    /// Length of auxiliary data.
    pub mr_datalen: u8,
    /// Number of sources.
    pub mr_numsrc: u16,
    /// Address being reported.
    pub mr_addr: In6Addr,
}

/// \[a\] `mld6_timers_are_running`: shortcut for fast timer.
pub static MLD6_TIMERS_ARE_RUNNING: AtomicI32 = AtomicI32::new(0);

/// `MLD_MRC_EXP(x)`: the exponent of a network-order Maximum Response Code.
pub const fn mld_mrc_exp(x: u16) -> u16 {
    (ntohs(x) >> 12) & 0x0007
}

/// `MLD_MRC_MANT(x)`: the mantissa of a network-order Maximum Response Code.
pub const fn mld_mrc_mant(x: u16) -> u16 {
    ntohs(x) & 0x0fff
}

/// `MLD_QQIC_EXP(x)`.
pub const fn mld_qqic_exp(x: u8) -> u8 {
    (x >> 4) & 0x07
}

/// `MLD_QQIC_MANT(x)`.
pub const fn mld_qqic_mant(x: u8) -> u8 {
    x & 0x0f
}

/// `MLD_QRESV(x)`.
pub const fn mld_qresv(x: u8) -> u8 {
    (x >> 4) & 0x0f
}

/// `MLD_SFLAG(x)`.
pub const fn mld_sflag(x: u8) -> u8 {
    (x >> 3) & 0x01
}

/// `MLD_QRV(x)`.
pub const fn mld_qrv(x: u8) -> u8 {
    x & 0x07
}

/// `hbh_buf`: the hop-by-hop options header `ip6_opts` points at (Router Alert), filled in by
/// `mld6_init` and read-only afterwards.
static MLD6_HBH_BUF: StaticCell<[u8; 8]> = StaticCell::new([0; 8]);

/// `ip6_opts`: the packet options of every MLD message: a hop-by-hop header with the Router
/// Alert option. Built from [`MLD6_HBH_BUF`] (see the module's deviations).
fn mld6_ip6_opts() -> Ip6Pktopts {
    let mut opts = Ip6Pktopts::default();
    ip6_initpktopts(&mut opts);
    opts.ip6po_hbh = NonNull::new(MLD6_HBH_BUF.as_ptr().cast::<Ip6Hbh>());
    opts
}

/// `mld6_init`: initializes the MLD timers and the router alert option.
pub fn mld6_init() {
    let rtalert_code = htons(IP6OPT_RTALERT_MLD);

    MLD6_TIMERS_ARE_RUNNING.store(0, Ordering::Relaxed);

    // ip6h_nxt will be fill in later; ip6h_len is 0: (8 >> 3) - 1.
    let mut hbh_buf = [0u8; 8];

    // XXX: grotty hard coding...
    hbh_buf[2] = IP6OPT_PADN; // 2 byte padding
    hbh_buf[3] = 0;
    hbh_buf[4] = IP6OPT_ROUTER_ALERT;
    hbh_buf[5] = (IP6OPT_RTALERT_LEN - 2) as u8;
    hbh_buf[6..8].copy_from_slice(&rtalert_code.to_ne_bytes());

    // SAFETY: `mld6_init` runs once, from `icmp6_init` on the boot CPU, before any MLD
    // message is sent or the cell is read.
    unsafe { MLD6_HBH_BUF.write(hbh_buf) };
}

/// `mld6_start_listening`: starts listening to group `in6m` on `ifp`; the report to send
/// (if any) is stored in `pkt` for `mld6_sendpkt` after the locks are released. Called holding
/// `if_maddrlock` for writing.
pub fn mld6_start_listening(in6m: &In6Multi, ifp: &Ifnet, pkt: &mut Mld6Pktinfo) {
    // XXX: These are necessary for KAME's link-local hack
    let mut all_nodes = IN6ADDR_LINKLOCAL_ALLNODES;
    let mut running = false;

    rw_assert_wrlock(&ifp.if_maddrlock);

    // RFC2710 page 10:
    // The node never sends a Report or Done for the link-scope all-nodes address.
    // MLD messages are never sent for multicast addresses whose scope is 0 (reserved) or 1
    // (node-local).
    all_nodes.set_s6_addr16(1, htons(in6m.in6m_ifidx().get() as u16));
    if in6_are_addr_equal(&in6m.in6m_addr(), &all_nodes)
        || __ipv6_addr_mc_scope(&in6m.in6m_addr()) < __IPV6_ADDR_SCOPE_LINKLOCAL
    {
        in6m.in6m_state.set(MLD_OTHERLISTENER);
        in6m.in6m_timer.set(0);
    } else {
        in6m.in6m_state.set(MLD_IREPORTEDLAST);
        in6m.in6m_timer
            .set(mld_random_delay(MLD_V1_MAX_RI * PR_FASTHZ as u32));
        pkt.mpi_addr = in6m.in6m_addr();
        pkt.mpi_rdomain = ifp.if_rdomain.get();
        pkt.mpi_ifidx = in6m.in6m_ifidx().get();
        pkt.mpi_type = i32::from(MLD_LISTENER_REPORT);
        running = true;
    }

    if running {
        MLD6_TIMERS_ARE_RUNNING.store(1, Ordering::Relaxed);
    }
}

/// `mld6_stop_listening`: stops listening to group `in6m` on `ifp`; the done message to
/// send (if any) is stored in `pkt`. Called holding `if_maddrlock`.
pub fn mld6_stop_listening(in6m: &In6Multi, ifp: &Ifnet, pkt: &mut Mld6Pktinfo) {
    // XXX: These are necessary for KAME's link-local hack
    let mut all_nodes = IN6ADDR_LINKLOCAL_ALLNODES;
    let mut all_routers = IN6ADDR_LINKLOCAL_ALLROUTERS;

    rw_assert_anylock(&ifp.if_maddrlock);

    all_nodes.set_s6_addr16(1, htons(in6m.in6m_ifidx().get() as u16));
    // XXX: necessary when mrouting
    all_routers.set_s6_addr16(1, htons(in6m.in6m_ifidx().get() as u16));

    if in6m.in6m_state.get() == MLD_IREPORTEDLAST
        && !in6_are_addr_equal(&in6m.in6m_addr(), &all_nodes)
        && __ipv6_addr_mc_scope(&in6m.in6m_addr()) > __IPV6_ADDR_SCOPE_INTFACELOCAL
    {
        pkt.mpi_addr = all_routers;
        pkt.mpi_rdomain = ifp.if_rdomain.get();
        pkt.mpi_ifidx = in6m.in6m_ifidx().get();
        pkt.mpi_type = i32::from(MLD_LISTENER_DONE);
    }
}

/// The `AF_INET6` multicast records of `ifp` (`TAILQ_FOREACH(ifma, &ifp->if_maddrlist,
/// ifma_list)` with the family test and `ifmatoin6m`). Called holding `if_maddrlock`.
fn inet6_memberships(ifp: &Ifnet) -> impl Iterator<Item = &In6Multi> {
    ifp.if_maddrlist.iter().filter_map(|ifma| {
        let sa = ifma.ifma_addr.get();
        // SAFETY: a record's address is readable (its protocol set it).
        (!sa.is_null() && unsafe { (*sa).sa_family } == AF_INET6).then(|| ifmatoin6m(ifma))
    })
}

/// `mld6_input`: a received MLD message at offset `off` of `m` (consumed): a query starts the
/// timers of our groups (or answers at once), a report from another listener stops the timer
/// of its group.
pub fn mld6_input(m: &'static Mbuf, off: i32) {
    let mut running = false;
    // XXX: These are necessary for KAME's link-local hack
    let mut all_nodes = IN6ADDR_LINKLOCAL_ALLNODES;

    let mut mp = Some(m);
    let Some(p) = ip6_exthdr_get(&mut mp, off, size_of::<MldHdr>() as i32) else {
        icmp6stat_inc(Icmp6statCounters::Icp6sTooshort);
        return;
    };
    // SAFETY: ip6_exthdr_get made the `MldHdr`'s bytes contiguous and readable at `p`; it is
    // plain data. The scope tricks of the C (`mld_addr.s6_addr16[1]` set, then cleared again)
    // work on this copy instead of the packet.
    let mldh: MldHdr = unsafe { ptr::read_unaligned(p.cast::<MldHdr>()) };
    let mut mld_addr = mldh.mld_addr;

    // source address validation
    let ip6 = mtod_ip6(m); // in case mpullup
    if !in6_is_addr_linklocal(&ip6.ip6_src) {
        // spec (RFC2710) does not explicitly specify to discard the packet from a non
        // link-local source address. But we believe it's expected to do so.
        m_freem(m);
        return;
    }

    let Some(ifp) = if_get(m.m_pkthdr().ph_ifidx.get()) else {
        m_freem(m);
        return;
    };

    // In the MLD6 specification, there are 3 states and a flag.
    //
    // In Non-Listener state, we simply don't have a membership record.
    // In Delaying Listener state, our timer is running (in6m->in6m_timer)
    // In Idle Listener state, our timer is not running (in6m->in6m_timer==0)
    //
    // The flag is in6m->in6m_state, it is set to MLD_OTHERLISTENER if we have heard a report
    // from another member, or MLD_IREPORTEDLAST if we sent the last report.
    match mldh.mld_type() {
        MLD_LISTENER_QUERY => 'query: {
            if ifp.if_flags.get() & IFF_LOOPBACK != 0 {
                break 'query;
            }

            if !in6_is_addr_unspecified(&mld_addr) && !in6_is_addr_multicast(&mld_addr) {
                break 'query; // print error or log stat?
            }
            if in6_is_addr_mc_linklocal(&mld_addr) {
                mld_addr.set_s6_addr16(1, htons(ifp.if_index.get() as u16)); // XXX
            }

            // - Start the timers in all of our membership records that the query applies to
            //   for the interface on which the query arrived excl. those that belong to the
            //   "all-nodes" group (ff02::1).
            // - Restart any timer that is already running but has A value longer than the
            //   requested timeout.
            // - Use the value specified in the query message as the maximum timeout.
            //
            // XXX: System timer resolution is too low to handle Max Response Delay, so set 1
            // to the internal timer even if the calculated value equals to zero when Max
            // Response Delay is positive.
            let mut timer =
                u32::from(ntohs(mldh.mld_maxdelay())) * PR_FASTHZ as u32 / MLD_TIMER_SCALE;
            if timer == 0 && mldh.mld_maxdelay() != 0 {
                timer = 1;
            }
            all_nodes.set_s6_addr16(1, htons(ifp.if_index.get() as u16));

            let mut pktlist: Vec<Mld6Pktinfo> = Vec::new();
            rw_enter_write(&ifp.if_maddrlock);
            for in6m in inet6_memberships(ifp) {
                if in6_are_addr_equal(&in6m.in6m_addr(), &all_nodes)
                    || __ipv6_addr_mc_scope(&in6m.in6m_addr()) < __IPV6_ADDR_SCOPE_LINKLOCAL
                {
                    continue;
                }

                if in6_is_addr_unspecified(&mld_addr)
                    || in6_are_addr_equal(&mld_addr, &in6m.in6m_addr())
                {
                    if timer == 0 {
                        // send a report immediately
                        in6m.in6m_state.set(MLD_IREPORTEDLAST);
                        in6m.in6m_timer.set(0); // reset timer
                        pktlist.push(Mld6Pktinfo {
                            mpi_addr: in6m.in6m_addr(),
                            mpi_rdomain: ifp.if_rdomain.get(),
                            mpi_ifidx: in6m.in6m_ifidx().get(),
                            mpi_type: i32::from(MLD_LISTENER_REPORT),
                        });
                    } else if in6m.in6m_timer.get() == 0 /* idle */
                        || in6m.in6m_timer.get() > timer
                    {
                        in6m.in6m_timer.set(mld_random_delay(timer));
                        running = true;
                    }
                }
            }
            rw_exit_write(&ifp.if_maddrlock);

            for pkt in &pktlist {
                mld6_sendpkt(pkt);
            }
        }
        MLD_LISTENER_REPORT => 'report: {
            // For fast leave to work, we have to know that we are the last person to send a
            // report for this group. Reports can potentially get looped back if we are a
            // multicast router, so discard reports sourced by me. Note that it is impossible
            // to check IFF_LOOPBACK flag of ifp for this purpose, since ip6_mloopback pass
            // the physical interface to if_input_local().
            if m.m_flags().get() & M_LOOP != 0 {
                // XXX: grotty flag, but efficient
                break 'report;
            }

            if !in6_is_addr_multicast(&mld_addr) {
                break 'report;
            }

            if in6_is_addr_mc_linklocal(&mld_addr) {
                mld_addr.set_s6_addr16(1, htons(ifp.if_index.get() as u16)); // XXX
            }
            // If we belong to the group being reported, stop our timer for that group.
            rw_enter_write(&ifp.if_maddrlock);
            if let Some(in6m) = in6_lookupmulti(&mld_addr, ifp) {
                in6m.in6m_state.set(MLD_OTHERLISTENER); // clear flag
                in6m.in6m_timer.set(0); // transit to idle state
            }
            rw_exit_write(&ifp.if_maddrlock);
        }
        // this is impossible: icmp6_input() only passes queries and reports.
        _ => {}
    }

    if running {
        MLD6_TIMERS_ARE_RUNNING.store(1, Ordering::Relaxed);
    }

    if_put(ifp);
    m_freem(m);
}

/// `mld6_fasttimo`: the MLD fast timeout: counts the membership timers down and sends the
/// reports of those that expire.
pub fn mld6_fasttimo() {
    let mut running = false;

    // Quick check to see if any work needs to be done, in order to minimize the overhead of
    // fasttimo processing. Variable mld6_timers_are_running is read atomically, but without
    // lock intentionally. In case it is not set due to MP races, we may miss to check the
    // timers. Then run the loop at next fast timeout.
    if MLD6_TIMERS_ARE_RUNNING.load(Ordering::Relaxed) == 0 {
        return;
    }
    MLD6_TIMERS_ARE_RUNNING.store(0, Ordering::Relaxed);

    net_lock_shared();

    let mut pktlist: Vec<Mld6Pktinfo> = Vec::new();
    for ifp in IFNETLIST.0.iter() {
        if mld6_checktimer(ifp, &mut pktlist) {
            running = true;
        }
    }

    for pkt in &pktlist {
        mld6_sendpkt(pkt);
    }

    net_unlock_shared();

    if running {
        MLD6_TIMERS_ARE_RUNNING.store(1, Ordering::Relaxed);
    }
}

/// `mld6_checktimer`: one tick of the timers of `ifp`'s groups; queues the reports of those
/// that expire on `pktlist`. `true` while some still run.
pub fn mld6_checktimer(ifp: &Ifnet, pktlist: &mut Vec<Mld6Pktinfo>) -> bool {
    let mut running = false;

    rw_enter_write(&ifp.if_maddrlock);
    for in6m in inet6_memberships(ifp) {
        if in6m.in6m_timer.get() == 0 {
            // do nothing
        } else {
            in6m.in6m_timer.set(in6m.in6m_timer.get() - 1);
            if in6m.in6m_timer.get() == 0 {
                in6m.in6m_state.set(MLD_IREPORTEDLAST);
                pktlist.push(Mld6Pktinfo {
                    mpi_addr: in6m.in6m_addr(),
                    mpi_rdomain: ifp.if_rdomain.get(),
                    mpi_ifidx: in6m.in6m_ifidx().get(),
                    mpi_type: i32::from(MLD_LISTENER_REPORT),
                });
            } else {
                running = true;
            }
        }
    }
    rw_exit_write(&ifp.if_maddrlock);

    running
}

/// `mld6_sendpkt`: sends the MLD message `pkt` describes: a hop limit 1 packet from a
/// link-local address of the interface (the unspecified one while it is tentative), with the
/// Router Alert option.
pub fn mld6_sendpkt(pkt: &Mld6Pktinfo) {
    const _: () = assert!(size_of::<Ip6Hdr>() + size_of::<MldHdr>() <= MHLEN);

    let Some(ifp) = if_get(pkt.mpi_ifidx) else {
        return;
    };

    // At first, find a link local address on the outgoing interface to use as the source
    // address of the MLD packet. We do not reject tentative addresses for MLD report to deal
    // with the case where we first join a link-local address.
    let ignflags = IN6_IFF_DUPLICATED | IN6_IFF_ANYCAST;
    let Some(ia6) = in6ifa_ifpforlinklocal(ifp, ignflags) else {
        if_put(ifp);
        return;
    };
    let ia6 = if ia6.ia6_flags.get() & IN6_IFF_TENTATIVE != 0 {
        None
    } else {
        Some(ia6)
    };

    // Allocate mbufs to store ip6 header and MLD header. We allocate 2 mbufs and make chain
    // in advance because it is more convenient when inserting the hop-by-hop option later.
    let Some(mh) = m_gethdr(M_DONTWAIT, MT_HEADER) else {
        if_put(ifp);
        return;
    };
    let Some(md) = m_get(M_DONTWAIT, MT_DATA) else {
        m_free(mh);
        if_put(ifp);
        return;
    };
    mh.m_next().set(Some(md));

    mh.m_pkthdr().ph_rtableid.set(pkt.mpi_rdomain);
    mh.m_pkthdr()
        .len
        .set((size_of::<Ip6Hdr>() + size_of::<MldHdr>()) as i32);
    mh.m_len().set(size_of::<Ip6Hdr>() as u32);
    m_align(mh, size_of::<Ip6Hdr>() as i32);

    // fill in the ip6 header
    let mut ip6 = Ip6Hdr::zeroed();
    ip6.ip6_flow = 0;
    ip6.set_ip6_vfc(IPV6_VERSION);
    // ip6_plen will be set later
    ip6.ip6_nxt = IPPROTO_ICMPV6 as u8;
    // ip6_hlim will be set by im6o.im6o_hlim
    ip6.ip6_src = ia6.map_or(IN6ADDR_ANY, ia6_in6);
    ip6.ip6_dst = pkt.mpi_addr;
    mtod_ip6_store(mh, &ip6);

    // fill in the MLD header
    md.m_len().set(size_of::<MldHdr>() as u32);
    let mut mldh = MldHdr {
        mld_icmp6_hdr: Icmp6Hdr::zeroed(),
        mld_addr: pkt.mpi_addr,
    };
    mldh.set_mld_type(pkt.mpi_type as u8);
    mldh.set_mld_code(0);
    mldh.set_mld_cksum(0);
    // XXX: we assume the function will not be called for query messages
    mldh.set_mld_maxdelay(0);
    mldh.set_mld_reserved(0);
    if in6_is_addr_mc_linklocal(&mldh.mld_addr) {
        mldh.mld_addr.set_s6_addr16(1, 0); // XXX
    }
    // SAFETY: a fresh mbuf has MLEN bytes at its data, more than an `MldHdr`.
    unsafe { ptr::write_unaligned(mtod::<MldHdr>(md), mldh) };
    mh.m_pkthdr()
        .csum_flags
        .set(mh.m_pkthdr().csum_flags.get() | M_ICMP_CSUM_OUT);

    // construct multicast option
    let im6o = Ip6Moptions {
        im6o_memberships: ListHead::new(),
        im6o_ifidx: pkt.mpi_ifidx as u16,
        im6o_hlim: 1,
        // Request loopback of the report if we are acting as a multicast router, so that the
        // process-level routing daemon can hear it. MROUTING: ip6_mrouter_active; not
        // configured.
        im6o_loop: 0,
    };
    if_put(ifp);

    icmp6stat_inc_hist(Icmp6statCounters::Icp6sOuthist, pkt.mpi_type as u8);
    let _ = ip6_output(
        mh,
        Some(&mld6_ip6_opts()),
        None,
        if ia6.is_some() { 0 } else { IPV6_UNSPECSRC },
        Some(&im6o),
        None,
    );
}

// The sizes of the C's `__packed` structures.
const _: () = {
    assert!(size_of::<Mldv2Query>() == 28);
    assert!(size_of::<Mldv2Report>() == 8);
    assert!(size_of::<Mldv2Record>() == 20);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for MLD: the message field macros, and the listener's timer arithmetic and state
    // transitions on a crafted query, a report and the fast timeout.

    use std::sync::MutexGuard;
    use std::vec::Vec;

    use super::*;
    use crate::net::if_::tests::test_packet;
    use crate::netinet::icmp6::Icmp6statCounters;
    use crate::netinet::ip_input::tests::setup as setup_ip;
    use crate::netinet::ip6::Ip6Hdr;
    use crate::netinet6::icmp6::{ICMP6COUNTERS, icmp6_init};
    use crate::netinet6::in6::tests::{a6, test_ia6, test_if};
    use crate::netinet6::in6::{
        IN6ADDR_LINKLOCAL_ALLNODES, in6_addmulti, in6_is_addr_mc_linklocal,
    };
    use crate::netinet6::nd6::{nd6_ifattach, nd6_init};
    use crate::sys::errno::Errno;
    use crate::sys::systm::{net_lock, net_unlock};

    #[test]
    fn query_fields() {
        // Maximum Response Code 0x7123: exponent 7, mantissa 0x123.
        assert_eq!(mld_mrc_exp(htons(0x7123)), 7);
        assert_eq!(mld_mrc_mant(htons(0x7123)), 0x123);
        // misc: resv 0xa, S 1, QRV 2.
        let misc = 0xa0 | 0x08 | 0x02;
        assert_eq!(
            (mld_qresv(misc), mld_sflag(misc), mld_qrv(misc)),
            (0xa, 1, 2)
        );
        assert_eq!((mld_qqic_exp(0x9c), mld_qqic_mant(0x9c)), (1, 0xc));
        assert_eq!(MLD_MINLEN, 8);
        assert_eq!(MLD_V2_QUERY_MINLEN, 28);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/netinet6/mld6.h");
        crate::reftest::assert_defines!(defs;
        MLD_V2_REPORT_MAXRECS, MLD_DO_NOTHING, MLD_MODE_IS_INCLUDE, MLD_MODE_IS_EXCLUDE,
        MLD_CHANGE_TO_INCLUDE_MODE, MLD_CHANGE_TO_EXCLUDE_MODE, MLD_ALLOW_NEW_SOURCES,
        MLD_BLOCK_OLD_SOURCES, MLD_V2_GENERAL_QUERY, MLD_V2_GROUP_QUERY,
        MLD_V2_GROUP_SOURCE_QUERY, MLD_V1_MAX_RI, MLD_TIMER_SCALE);
    }

    /// An interface with a link-local address (which MLD reports come from) and the MLD state.
    /// Its MTU is 0: `ip6_output` drops (EMSGSIZE) what it is given, because `if_output_tso` has no
    /// AF_INET6 case yet (phase 2 of the INET6 port) and would panic.
    fn mld_if() -> (
        (MutexGuard<'static, ()>, MutexGuard<'static, ()>),
        &'static Ifnet,
    ) {
        let g = setup_ip();
        nd6_init();
        icmp6_init();
        let ifp = test_if(b"tml0");
        ifp.if_ioctl.set(Some(accepting_ioctl));
        nd6_ifattach(ifp);
        test_ia6(ifp, ll(ifp), 0);
        ifp.if_mtu.set(0);
        MLD6_TIMERS_ARE_RUNNING.store(0, Ordering::Relaxed);
        (g, ifp)
    }

    /// `fe80::5054:ff:fe00:1` with the zone of `ifp` embedded.
    fn ll(ifp: &Ifnet) -> In6Addr {
        let mut a = a6("fe80::5054:ff:fe00:1");
        a.set_s6_addr16(1, htons(ifp.if_index.get() as u16));
        a
    }

    /// A group address with `ifp`'s zone embedded when it has a link-local scope.
    fn group(ifp: &Ifnet, s: &str) -> In6Addr {
        let mut g = a6(s);
        if in6_is_addr_mc_linklocal(&g) {
            g.set_s6_addr16(1, htons(ifp.if_index.get() as u16));
        }
        g
    }

    /// `ifp` joins `g`.
    fn join(ifp: &'static Ifnet, g: In6Addr) -> &'static In6Multi {
        net_lock();
        let in6m = in6_addmulti(&g, ifp).expect("join");
        net_unlock();
        in6m
    }

    /// The count of MLD messages of `type_` sent.
    fn sent(type_: u8) -> u64 {
        ICMP6COUNTERS[Icmp6statCounters::Icp6sOuthist as usize + usize::from(type_)]
            .load(Ordering::Relaxed)
    }

    fn tooshort() -> u64 {
        ICMP6COUNTERS[Icmp6statCounters::Icp6sTooshort as usize].load(Ordering::Relaxed)
    }

    /// A received MLD message `type_` from `src` for group `addr`, maximum response delay
    /// `maxdelay` (in milliseconds), on `ifp`.
    fn mld_message(
        ifp: &Ifnet,
        src: In6Addr,
        type_: u8,
        maxdelay: u16,
        addr: In6Addr,
    ) -> &'static Mbuf {
        let mut ip6 = Ip6Hdr::zeroed();
        ip6.set_ip6_vfc(IPV6_VERSION);
        ip6.ip6_plen = htons(size_of::<MldHdr>() as u16);
        ip6.ip6_nxt = IPPROTO_ICMPV6 as u8;
        ip6.ip6_hlim = 1;
        ip6.ip6_src = src;
        ip6.ip6_dst = IN6ADDR_LINKLOCAL_ALLNODES;
        let mut mldh = MldHdr {
            mld_icmp6_hdr: Icmp6Hdr::zeroed(),
            mld_addr: addr,
        };
        mldh.set_mld_type(type_);
        mldh.set_mld_maxdelay(htons(maxdelay));
        let mut b = std::vec![0u8; size_of::<Ip6Hdr>() + size_of::<MldHdr>()];
        // SAFETY: plain-data headers, no padding, copied into a buffer of their size.
        unsafe {
            ptr::copy_nonoverlapping(
                ptr::from_ref(&ip6).cast::<u8>(),
                b.as_mut_ptr(),
                size_of::<Ip6Hdr>(),
            );
            ptr::copy_nonoverlapping(
                ptr::from_ref(&mldh).cast::<u8>(),
                b.as_mut_ptr().add(size_of::<Ip6Hdr>()),
                size_of::<MldHdr>(),
            );
        }
        let m = test_packet(&b);
        m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
        m
    }

    #[test]
    fn joining_a_group_starts_a_delayed_report() {
        let (_g, ifp) = mld_if();
        let reports = sent(MLD_LISTENER_REPORT);

        let g = group(ifp, "ff02::1234");
        let in6m = join(ifp, g);
        // The state says we reported last, the timer is a random delay of up to 10 seconds of
        // fast timeouts, and the first report went out with the join.
        assert_eq!(in6m.in6m_state.get(), MLD_IREPORTEDLAST);
        assert!((1..=MLD_V1_MAX_RI * PR_FASTHZ as u32).contains(&in6m.in6m_timer.get()));
        assert_eq!(MLD6_TIMERS_ARE_RUNNING.load(Ordering::Relaxed), 1);
        assert_eq!(sent(MLD_LISTENER_REPORT), reports + 1);

        // The all-nodes group, and the groups of interface-local and smaller scope, are never
        // reported.
        MLD6_TIMERS_ARE_RUNNING.store(0, Ordering::Relaxed);
        for s in ["ff02::1", "ff01::5", "ff01::1"] {
            let in6m = join(ifp, group(ifp, s));
            assert_eq!(in6m.in6m_state.get(), MLD_OTHERLISTENER, "{s}");
            assert_eq!(in6m.in6m_timer.get(), 0, "{s}");
        }
        assert_eq!(sent(MLD_LISTENER_REPORT), reports + 1);
        assert_eq!(MLD6_TIMERS_ARE_RUNNING.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn leaving_sends_done_only_if_we_reported_last() {
        let (_g, ifp) = mld_if();
        let g = group(ifp, "ff02::1234");
        let in6m = join(ifp, g);
        let all = join(ifp, group(ifp, "ff02::1"));
        let mut all_routers = IN6ADDR_LINKLOCAL_ALLROUTERS;
        all_routers.set_s6_addr16(1, htons(ifp.if_index.get() as u16));

        rw_enter_write(&ifp.if_maddrlock);
        // We reported last: a Done message to the all-routers group.
        let mut pkt = Mld6Pktinfo::default();
        mld6_stop_listening(in6m, ifp, &mut pkt);
        assert_eq!(
            pkt,
            Mld6Pktinfo {
                mpi_addr: all_routers,
                mpi_rdomain: ifp.if_rdomain.get(),
                mpi_ifidx: ifp.if_index.get(),
                mpi_type: i32::from(MLD_LISTENER_DONE),
            }
        );

        // Somebody else reported after us: nothing.
        in6m.in6m_state.set(MLD_OTHERLISTENER);
        let mut pkt = Mld6Pktinfo::default();
        mld6_stop_listening(in6m, ifp, &mut pkt);
        assert_eq!(pkt, Mld6Pktinfo::default());

        // The all-nodes group is never left loudly.
        all.in6m_state.set(MLD_IREPORTEDLAST);
        let mut pkt = Mld6Pktinfo::default();
        mld6_stop_listening(all, ifp, &mut pkt);
        assert_eq!(pkt, Mld6Pktinfo::default());
        rw_exit_write(&ifp.if_maddrlock);
    }

    #[test]
    fn the_fast_timeout_counts_down_and_reports() {
        let (_g, ifp) = mld_if();
        let in6m = join(ifp, group(ifp, "ff02::1234"));
        let reports = sent(MLD_LISTENER_REPORT);

        // No timer running: nothing to do.
        in6m.in6m_timer.set(2);
        in6m.in6m_state.set(MLD_OTHERLISTENER);
        MLD6_TIMERS_ARE_RUNNING.store(0, Ordering::Relaxed);
        mld6_fasttimo();
        assert_eq!(
            in6m.in6m_timer.get(),
            2,
            "the shortcut says no timer is running"
        );

        // Two ticks: the first only counts, the second reports.
        MLD6_TIMERS_ARE_RUNNING.store(1, Ordering::Relaxed);
        mld6_fasttimo();
        assert_eq!(in6m.in6m_timer.get(), 1);
        assert_eq!(
            MLD6_TIMERS_ARE_RUNNING.load(Ordering::Relaxed),
            1,
            "still running"
        );
        assert_eq!(sent(MLD_LISTENER_REPORT), reports);
        mld6_fasttimo();
        assert_eq!(in6m.in6m_timer.get(), 0);
        assert_eq!(in6m.in6m_state.get(), MLD_IREPORTEDLAST);
        assert_eq!(sent(MLD_LISTENER_REPORT), reports + 1);
        assert_eq!(
            MLD6_TIMERS_ARE_RUNNING.load(Ordering::Relaxed),
            0,
            "idle again"
        );

        // checktimer on its own: the report is queued, not sent.
        in6m.in6m_timer.set(1);
        let mut pktlist = Vec::new();
        assert!(!mld6_checktimer(ifp, &mut pktlist));
        assert_eq!(pktlist.len(), 1);
        assert_eq!(pktlist[0].mpi_addr, in6m.in6m_addr());
        assert_eq!(pktlist[0].mpi_type, i32::from(MLD_LISTENER_REPORT));
    }

    #[test]
    fn a_query_sets_the_timers_of_the_groups_it_asks_about() {
        let (_g, ifp) = mld_if();
        let a = join(ifp, group(ifp, "ff02::1234"));
        let b = join(ifp, group(ifp, "ff02::5678"));
        let all = join(ifp, group(ifp, "ff02::1"));
        let querier = ll(ifp);
        let reset = || {
            for x in [a, b] {
                x.in6m_timer.set(0);
                x.in6m_state.set(MLD_OTHERLISTENER);
            }
            MLD6_TIMERS_ARE_RUNNING.store(0, Ordering::Relaxed);
        };
        let query = |maxdelay: u16, g: In6Addr| {
            mld6_input(
                mld_message(ifp, querier, MLD_LISTENER_QUERY, maxdelay, g),
                40,
            );
        };

        // A general query (group ::) with a 10 s maximum response delay: 50 fast ticks at most.
        reset();
        query(10_000, IN6ADDR_ANY);
        for x in [a, b] {
            assert!((1..=50).contains(&x.in6m_timer.get()));
        }
        assert_eq!(all.in6m_timer.get(), 0, "never the all-nodes group");
        assert_eq!(MLD6_TIMERS_ARE_RUNNING.load(Ordering::Relaxed), 1);

        // A running timer longer than the response delay is restarted with a shorter one; a
        // shorter one is kept.
        a.in6m_timer.set(40);
        b.in6m_timer.set(1);
        query(1_000, IN6ADDR_ANY);
        assert!((1..=5).contains(&a.in6m_timer.get()));
        assert_eq!(b.in6m_timer.get(), 1);

        // A group-specific query only touches that group. 100 ms is less than a tick: 1.
        reset();
        query(100, group(ifp, "ff02::5678"));
        assert_eq!((a.in6m_timer.get(), b.in6m_timer.get()), (0, 1));

        // A maximum response delay of 0 asks for an answer at once.
        reset();
        let reports = sent(MLD_LISTENER_REPORT);
        query(0, IN6ADDR_ANY);
        assert_eq!((a.in6m_timer.get(), b.in6m_timer.get()), (0, 0));
        assert_eq!(a.in6m_state.get(), MLD_IREPORTEDLAST);
        assert_eq!(sent(MLD_LISTENER_REPORT), reports + 2, "one per group");
        assert_eq!(all.in6m_state.get(), MLD_OTHERLISTENER);
    }

    #[test]
    fn queries_from_the_wrong_place_or_about_the_wrong_thing_are_ignored() {
        let (_g, ifp) = mld_if();
        let a = join(ifp, group(ifp, "ff02::1234"));
        a.in6m_timer.set(0);
        a.in6m_state.set(MLD_OTHERLISTENER);
        MLD6_TIMERS_ARE_RUNNING.store(0, Ordering::Relaxed);
        let untouched = |what: &str| {
            assert_eq!(a.in6m_timer.get(), 0, "{what}");
            assert_eq!(MLD6_TIMERS_ARE_RUNNING.load(Ordering::Relaxed), 0, "{what}");
        };

        // Not from a link-local address.
        mld6_input(
            mld_message(
                ifp,
                a6("fd00:77::2"),
                MLD_LISTENER_QUERY,
                10_000,
                IN6ADDR_ANY,
            ),
            40,
        );
        untouched("a global source");

        // About a unicast address.
        mld6_input(
            mld_message(ifp, ll(ifp), MLD_LISTENER_QUERY, 10_000, a6("fd00:77::2")),
            40,
        );
        untouched("a unicast group");

        // Shorter than an MLD header: counted, nothing else.
        let before = tooshort();
        let m = mld_message(ifp, ll(ifp), MLD_LISTENER_QUERY, 10_000, IN6ADDR_ANY);
        m.m_len().set(60);
        m.m_pkthdr().len.set(60);
        mld6_input(m, 40);
        assert_eq!(tooshort(), before + 1);
        untouched("a short message");
    }

    #[test]
    fn a_report_from_another_listener_stops_our_timer() {
        let (_g, ifp) = mld_if();
        let a = join(ifp, group(ifp, "ff02::1234"));
        let other = group(ifp, "ff02::9999");
        let src = a6("fe80::5054:ff:fe00:7");
        let report = |g: In6Addr| mld_message(ifp, src, MLD_LISTENER_REPORT, 0, g);

        // A report for our group: we stop reporting it (somebody else did).
        a.in6m_timer.set(30);
        a.in6m_state.set(MLD_IREPORTEDLAST);
        mld6_input(report(group(ifp, "ff02::1234")), 40);
        assert_eq!(
            (a.in6m_timer.get(), a.in6m_state.get()),
            (0, MLD_OTHERLISTENER)
        );

        // For a group we are not in: nothing; for a non-multicast address: nothing.
        a.in6m_timer.set(30);
        mld6_input(report(other), 40);
        mld6_input(report(a6("fd00:77::2")), 40);
        assert_eq!(a.in6m_timer.get(), 30);

        // A report that we looped back ourselves does not count.
        let m = report(group(ifp, "ff02::1234"));
        m.m_flags().set(m.m_flags().get() | M_LOOP);
        mld6_input(m, 40);
        assert_eq!(a.in6m_timer.get(), 30);
    }

    #[test]
    fn the_router_alert_option_is_ready_for_every_message() {
        let (_g, _ifp) = mld_if();
        let opts = mld6_ip6_opts();
        let hbh = opts.ip6po_hbh.expect("a hop-by-hop header");
        // SAFETY: the buffer is 8 bytes, written by `mld6_init` (`icmp6_init`) and read-only since.
        let b = unsafe { core::slice::from_raw_parts(hbh.as_ptr().cast::<u8>(), 8) };
        // next header (filled in by ip6_output), length 0 (8 bytes), PadN of 2, Router Alert
        // option (type 5, length 2) with value 0: MLD.
        assert_eq!(b[1..], [0, IP6OPT_PADN, 0, IP6OPT_ROUTER_ALERT, 2, 0, 0]);
        assert_eq!(opts.ip6po_hlim, -1);
        assert_eq!(opts.ip6po_tclass, -1);
    }

    /// A driver `ioctl` that accepts the multicast requests.
    ///
    /// # Safety
    ///
    /// `IfIoctlFn`'s contract.
    unsafe fn accepting_ioctl(_ifp: &'static Ifnet, cmd: u64, _data: *mut u8) -> Result<(), Errno> {
        use crate::sys::sockio::{SIOCADDMULTI, SIOCDELMULTI};
        match cmd {
            SIOCADDMULTI | SIOCDELMULTI => Ok(()),
            _ => Err(Errno::ENOTTY),
        }
    }
}
/* </TESTS> */
