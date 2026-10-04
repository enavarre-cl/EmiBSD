/*	$OpenBSD: mld6.h,v 1.2 2010/03/22 21:29:22 jsg Exp $	*/
/*	$FreeBSD: mld6.h,v 1.1 2009/04/29 11:31:23 bms Exp $	*/
/*	$OpenBSD: mld6.c,v 1.76 2026/09/17 15:56:59 bluhm Exp $	*/
/*	$KAME: mld6.c,v 1.26 2001/02/16 14:50:35 itojun Exp $	*/
/* <LICENSES> */
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

#[cfg(test)]
mod tests;
