/*	$OpenBSD: ip_input.c,v 1.433 2026/08/11 14:28:59 bluhm Exp $	*/
/*	$NetBSD: ip_input.c,v 1.30 1996/03/16 23:53:58 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1988, 1993
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
 *	@(#)ip_input.c	8.2 (Berkeley) 1/4/94
 */
/* </LICENSES> */

//! IPv4 input: header checks, options, local delivery through the protocol switch,
//! reassembly, forwarding and the `net.inet.ip` sysctls.
//!
//! Upstream: sys/netinet/ip_input.c @ 3ce1f3f79392
//!
//! `ipv4_input` (from `ether_input` or `if_input_local`) checksums and checks the header
//! (`ipv4_check`), processes options (`ip_dooptions`) and either delivers the datagram
//! (`in_ouraddr` says it is ours: `ip_ours` reassembles fragments and runs the protocols that
//! take the shared net lock, the rest go through `ipintrq` to `ipintr`) or forwards it
//! (`ip_forward`, only with `net.inet.ip.forwarding`, which is 0). `ip_deliver` walks the
//! protocol switch (`inetsw[ip_protox[nxt]]`).
//!
//! Locks: \[I\] immutable after creation, \[N\] net lock, \[Q\] `ipq_mutex`, \[a\] atomic
//! operations.
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - The sysctl variables are `AtomicI32` statics (`docs/C_TO_RUST.md`); `ipcounters`
//!   (`struct cpumem *`) is a static array of atomics (`IPCOUNTERS`), as `uvmexp` and `mbstat`
//!   are.
//! - `ip_input_if`, `ip_ours`, `ip_deliver` and the other `struct mbuf **` functions take
//!   `&mut Option<&'static Mbuf>` (`sys/protosw.rs`); `in_ouraddr`'s 0/1/2 is an `i32` as in C;
//!   `ip_dooptions` returns `true` where the C returns 1 (the packet was forwarded or freed).
//! - The IP header in the packet is read and written as a copy ([`mtod_ip`],
//!   [`mtod_ip_store`]): mbuf data has no 4-byte alignment guarantee, which `struct ip` needs
//!   in Rust; the reassembly queue keeps a raw pointer to each fragment's header, as the C
//!   does, and reads it unaligned the same way.
//! - `in_pcb.c` is not ported: `ip_init`'s `baddynamicports`/`rootonlyports` setup and the
//!   `net.inet.ip.porthifirst`.. sysctls (`ipport_*`) report themselves; `ip_savecontrol`
//!   (`struct inpcb`, `sbcreatecontrol`) waits for the socket layer.
//! - Not configured, each a comment at its site: `NPF` (`pf_test`, `pf_ouraddr`), `NCARP`
//!   (`carp_lsdrop`, `carp_strict_addr_chk`), `MROUTING` (`ip_mforward`,
//!   `ip_mrouter_active`, the `mrt` sysctls answer `EOPNOTSUPP` as the C's `#else` does),
//!   `IPSEC` (`ipsec_forward_check`, `ipsec_local_check`, `ipsec_init`, `ipsec_sysctl`) and
//!   `INET6` (`ip6_protox`, the IPv6 delivery loop).
//! - `ip_forward`'s 68-byte `icmp_buf` is an array on the stack, as in C.
//! - `KERNEL_LOCK()`/`KERNEL_UNLOCK()` are nothing without `MULTIPROCESSOR`.

use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;
use core::slice;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write};
use crate::kern::kern_sysctl::{
    SYSCTL_LOCK, sysctl_bounded_arr, sysctl_int_bounded, sysctl_rdint, sysctl_rdstruct,
    sysctl_securelevel_int,
};
use crate::kern::kern_task::task_add;
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::kern::uipc_domain::pffindproto;
use crate::kern::uipc_mbuf::{
    m_adj, m_calchdrlen, m_cat, m_copydata, m_freem, m_get, m_gethdr, m_pullup, m_removehdr,
    ml_dequeue, mq_delist, mq_enqueue, mq_init,
};
use crate::kern::uipc_mbuf2::{m_tag_delete, m_tag_find, m_tag_get, m_tag_prepend};
use crate::machine::intr::IPL_SOFTNET;
use crate::net::if_::{
    IFF_LOOPBACK, if_get, if_put, ifa_ifwithaddr, ifaof_ifpforaddr, net_tq, niq_enqueue,
};
use crate::net::if_types::IFT_ENC;
use crate::net::if_var::{Ifnet, Netstack, Niqueue, niq_dequeue, sysctl_niq};
use crate::net::netisr::NETISR_IP;
use crate::net::route::{
    RT_RESOLVE, RTF_BROADCAST, RTF_DYNAMIC, RTF_GATEWAY, RTF_LOCAL, RTF_MODIFIED, Route,
    route_mpath, rt_timer_queue_change, rt_timer_queue_flush, rtalloc, rtfree, rtisvalid,
};
use crate::net::rtable::{rt_key, rtable_l2};
use crate::netinet::if_ether::{ARPINQ, ARPT_DOWN, ARPT_KEEP, arpinit, arpproxy, la_hold_total};
use crate::netinet::in_::{
    IN_CLASSA_NSHIFT, IN_LOOPBACKNET, INADDR_ANY, INADDR_BROADCAST, IPCTL_ARPDOWN, IPCTL_ARPQUEUE,
    IPCTL_ARPQUEUED, IPCTL_ARPTIMEOUT, IPCTL_DEFTTL, IPCTL_DIRECTEDBCAST, IPCTL_FORWARDING,
    IPCTL_IFQUEUE, IPCTL_IPPORT_FIRSTAUTO, IPCTL_IPPORT_HIFIRSTAUTO, IPCTL_IPPORT_HILASTAUTO,
    IPCTL_IPPORT_LASTAUTO, IPCTL_IPPORT_MAXQUEUE, IPCTL_MFORWARDING, IPCTL_MRTMFC, IPCTL_MRTPROTO,
    IPCTL_MRTSTATS, IPCTL_MRTVIF, IPCTL_MTUDISC, IPCTL_MTUDISCTIMEOUT, IPCTL_MULTIPATH,
    IPCTL_SENDREDIRECTS, IPCTL_SOURCEROUTE, IPCTL_STATS, IPPROTO_DONE, IPPROTO_IPV4, IPPROTO_MAX,
    IPPROTO_RAW, InAddr, SockaddrIn, in_canforward, in_classfulbroadcast, in_hasmulti,
    in_local_group, in_multicast, sintosa,
};
use crate::netinet::in_cksum::in_cksum;
use crate::netinet::in_proto::{INETDOMAIN, INETSW, IP_PROTOX};
use crate::netinet::in_var::ifatoia;
use crate::netinet::ip::{
    IP_MAXPACKET, IP_MF, IP_OFFMASK, IPDEFTTL, IPFRAGTTL, IPOPT_EOL, IPOPT_LSRR, IPOPT_MINOFF,
    IPOPT_NOP, IPOPT_OFFSET, IPOPT_OLEN, IPOPT_OPTVAL, IPOPT_RR, IPOPT_SSRR, IPOPT_TS,
    IPOPT_TS_PRESPEC, IPOPT_TS_TSANDADDR, IPOPT_TS_TSONLY, IPQ_MAXLEN, IPTOS_ECN_CE,
    IPTOS_ECN_MASK, IPTOS_ECN_NOTECT, IPTTLDEC, IPVERSION, Ip, IpTimestamp,
};
use crate::netinet::ip_icmp::{
    ICMP_PARAMPROB, ICMP_REDIRECT, ICMP_REDIRECT_HOST, ICMP_TIMXCEED, ICMP_TIMXCEED_INTRANS,
    ICMP_UNREACH, ICMP_UNREACH_HOST, ICMP_UNREACH_NEEDFRAG, ICMP_UNREACH_SRCFAIL,
    IP_MTUDISC_TIMEOUT_Q, icmp_error, iptime,
};
use crate::netinet::ip_id::ip_randomid_init;
use crate::netinet::ip_output::ip_output;
use crate::netinet::ip_var::{
    IP_ALLOWBROADCAST, IP_FORWARDING, IP_FORWARDING_IPSEC, IP_RAWOUTPUT, IP_REDIRECT,
    IPMTUDISCTIMEOUT, Ipoffnxt, Ipq, IpqList, Ipqent, Ipstat, IpstatCounters, MAX_IPOPTLEN,
    ipstat_dec, ipstat_inc, mtod_ip, mtod_ip_store,
};
use crate::sys::endian::{htons, ntohl, ntohs};
use crate::sys::errno::Errno;
use crate::sys::limits::INT_MAX;
use crate::sys::malloc::M_NOWAIT;
use crate::sys::mbuf::{
    M_BCAST, M_COPYFLAGS, M_DONTWAIT, M_EXT, M_IPV4_CSUM_IN_BAD, M_IPV4_CSUM_IN_OK, M_MCAST,
    M_PKTHDR, MHLEN, MT_DATA, MT_SOOPTS, Mbuf, MbufList, MbufQueue, PACKET_TAG_IP_OFFNXT,
    PACKET_TAG_SRCROUTE, PF_TAG_GENERATED, PF_TAG_TRANSLATE_LOCALHOST, m_freemp, ml_empty, mtod,
};
use crate::sys::mutex::Mutex;
use crate::sys::pool::{PR_NOWAIT, Pool};
use crate::sys::protosw::{PR_MPINPUT, PRC_NCMDS, Protosw};
use crate::sys::queue::ListHead;
use crate::sys::socket::{AF_INET, AF_UNSPEC, PF_INET, SOCK_RAW};
use crate::sys::sysctl::SysctlBoundedArgs;
use crate::sys::systm::{
    net_assert_locked, net_lock, net_lock_shared, net_unlock, net_unlock_shared,
};
use crate::sys::task::Task;
use crate::unported;

/// `struct ip_srcrt`: the IP options of an incoming packet saved in case a protocol wants to
/// respond to it over the same route if it got here using IP source routing. This allows
/// connection establishment and maintenance when the remote end is on a network that is not
/// known to us.
#[repr(C)]
#[derive(Clone, Copy)]
struct IpSrcrt {
    /// `isr_nhops`: number of hops.
    isr_nhops: i32,
    /// `isr_dst`: final destination.
    isr_dst: InAddr,
    /// `isr_nop`: one NOP to align.
    isr_nop: u8,
    /// `isr_hdr`: OPTVAL, OLEN & OFFSET.
    isr_hdr: [u8; IPOPT_OFFSET + 1],
    /// `isr_routes`.
    isr_routes: [InAddr; MAX_IPOPTLEN / size_of::<InAddr>()],
}

/// `LIST_HEAD(, ipq) ipq`, made `Sync`: the reassembly queues, protected by `ipq_mutex`.
struct IpqHead(ListHead<IpqList>);

// SAFETY: see the type's doc.
unsafe impl Sync for IpqHead {}

/// \[a\] `ip_forwarding`: act as router (2: only IPsec processed packets).
#[allow(non_upper_case_globals)] // the C name; `IP_FORWARDING` is `ip_var.h`'s output flag
pub static ip_forwarding: AtomicI32 = AtomicI32::new(0);
/// \[a\] `ipmforwarding`.
pub static IPMFORWARDING: AtomicI32 = AtomicI32::new(0);
/// \[a\] `ipmultipath`.
pub static IPMULTIPATH: AtomicI32 = AtomicI32::new(0);
/// \[a\] `ip_sendredirects`.
pub static IP_SENDREDIRECTS: AtomicI32 = AtomicI32::new(1);
/// \[a\] `ip_dosourceroute`.
pub static IP_DOSOURCEROUTE: AtomicI32 = AtomicI32::new(0);
/// \[a\] `ip_defttl`: default IP ttl.
pub static IP_DEFTTL: AtomicI32 = AtomicI32::new(IPDEFTTL as i32);
/// \[a\] `ip_mtudisc`: mtu discovery.
#[allow(non_upper_case_globals)] // the C name; `IP_MTUDISC` is `ip_var.h`'s output flag
pub static ip_mtudisc: AtomicI32 = AtomicI32::new(1);
/// \[a\] `ip_mtudisc_timeout`: seconds to timeout mtu discovery.
pub static IP_MTUDISC_TIMEOUT: AtomicI32 = AtomicI32::new(IPMTUDISCTIMEOUT);
/// \[a\] `ip_directedbcast`: accept all broadcast packets.
pub static IP_DIRECTEDBCAST: AtomicI32 = AtomicI32::new(0);

/// `ipq_mutex`.
static IPQ_MUTEX: Mutex = Mutex::new(IPL_SOFTNET);

/// \[Q\] `ipq`: IP reassembly queue.
static IPQ: IpqHead = IpqHead(ListHead::new());

/// \[a\] `ip_maxqueue`: keep track of memory used for reassembly.
pub static IP_MAXQUEUE: AtomicI32 = AtomicI32::new(300);
/// \[Q\] `ip_frags`.
static IP_FRAGS: AtomicI32 = AtomicI32::new(0);

/// `ipctl_vars[]`.
static IPCTL_VARS: [SysctlBoundedArgs; 8] = [
    SysctlBoundedArgs::new(IPCTL_FORWARDING, &ip_forwarding, 0, 2),
    SysctlBoundedArgs::new(IPCTL_SENDREDIRECTS, &IP_SENDREDIRECTS, 0, 1),
    SysctlBoundedArgs::new(IPCTL_DIRECTEDBCAST, &IP_DIRECTEDBCAST, 0, 1),
    // MROUTING: IPCTL_MRTPROTO, read only; not configured.
    SysctlBoundedArgs::new(IPCTL_DEFTTL, &IP_DEFTTL, 0, 255),
    // IPCTL_IPPORT_FIRSTAUTO, _LASTAUTO, _HIFIRSTAUTO, _HILASTAUTO: in_pcb.c's variables,
    // reported in ip_sysctl.
    SysctlBoundedArgs::new(IPCTL_IPPORT_MAXQUEUE, &IP_MAXQUEUE, 0, 10000),
    SysctlBoundedArgs::new(IPCTL_MFORWARDING, &IPMFORWARDING, 0, 1),
    SysctlBoundedArgs::new(IPCTL_ARPTIMEOUT, &ARPT_KEEP, 0, INT_MAX),
    SysctlBoundedArgs::new(IPCTL_ARPDOWN, &ARPT_DOWN, 0, INT_MAX),
];

/// `ipintrq`.
pub static IPINTRQ: Niqueue = Niqueue::new(IPQ_MAXLEN as u32, NETISR_IP);

/// `ipqent_pool`.
pub static IPQENT_POOL: Pool = Pool::new();
/// `ipq_pool`.
static IPQ_POOL: Pool = Pool::new();

/// `ipcounters`: the IP statistics (see the module's deviations).
pub static IPCOUNTERS: [AtomicU64; IpstatCounters::IpsNcounters as usize] =
    [const { AtomicU64::new(0) }; IpstatCounters::IpsNcounters as usize];

/// `ipsend_mq`.
static IPSEND_MQ: MbufQueue = MbufQueue::new(64, IPL_SOFTNET);
/// `ipsendraw_mq`.
static IPSENDRAW_MQ: MbufQueue = MbufQueue::new(64, IPL_SOFTNET);

/// `ipsend_task`.
static IPSEND_TASK: Task = Task::new(ip_send_dispatch, (&raw const IPSEND_MQ).cast_mut().cast());
/// `ipsendraw_task`.
static IPSENDRAW_TASK: Task = Task::new(
    ip_sendraw_dispatch,
    (&raw const IPSENDRAW_MQ).cast_mut().cast(),
);

/// `inetctlerrmap[]`: the errno of each `PRC_*` control command.
pub static INETCTLERRMAP: [Option<Errno>; PRC_NCMDS] = [
    None,
    None,
    None,
    None,
    None,
    Some(Errno::EMSGSIZE),
    Some(Errno::EHOSTDOWN),
    Some(Errno::EHOSTUNREACH),
    Some(Errno::EHOSTUNREACH),
    Some(Errno::EHOSTUNREACH),
    Some(Errno::ECONNREFUSED),
    Some(Errno::ECONNREFUSED),
    Some(Errno::EMSGSIZE),
    Some(Errno::EHOSTUNREACH),
    None,
    None,
    None,
    None,
    None,
    None,
    Some(Errno::ENOPROTOOPT),
];

/// The index of `pr` in `inetsw[]` (`pr - inetsw`).
fn inetsw_index(pr: &Protosw) -> u8 {
    match INETSW.iter().position(|p| ptr::eq(p, pr)) {
        Some(i) => i as u8,
        None => panic(format_args!("ip_init: protocol not in inetsw")),
    }
}

/// `ip_init`: IP initialization: fill in IP protocol switch table. All protocols not
/// implemented in kernel go to raw IP protocol handler.
pub fn ip_init() {
    ip_randomid_init();

    // ipcounters = counters_alloc(ips_ncounters): a static (the module's deviations).

    pool_init(
        &IPQENT_POOL,
        size_of::<Ipqent>(),
        0,
        IPL_SOFTNET,
        0,
        "ipqe",
        None,
    );
    pool_init(&IPQ_POOL, size_of::<Ipq>(), 0, IPL_SOFTNET, 0, "ipq", None);

    let Some(pr) = pffindproto(i32::from(PF_INET), IPPROTO_RAW, SOCK_RAW) else {
        panic(format_args!("ip_init"));
    };
    let raw = inetsw_index(pr);
    for p in &IP_PROTOX {
        p.store(raw, Ordering::Relaxed);
    }
    for pr in INETDOMAIN.dom_protosw {
        if pr.pr_domain.dom_family == i32::from(PF_INET)
            && pr.pr_protocol != 0
            && i32::from(pr.pr_protocol) != IPPROTO_RAW
            && i32::from(pr.pr_protocol) < IPPROTO_MAX
        {
            IP_PROTOX[pr.pr_protocol as usize].store(inetsw_index(pr), Ordering::Relaxed);
        }
    }
    IPQ.0.init();

    // Fill in list of ports not to allocate dynamically, and of ports only root can bind to:
    // baddynamicports and rootonlyports are in_pcb.c's.
    let _ = unported!("baddynamicports, rootonlyports (netinet/in_pcb.c)");

    mq_init(&IPSEND_MQ, 64, IPL_SOFTNET);
    mq_init(&IPSENDRAW_MQ, 64, IPL_SOFTNET);

    // NETHER > 0
    arpinit();
    // IPSEC: ipsec_init(); MROUTING: mrt_init(); not configured.
}

/// `ip_ours`: enqueue packet for local delivery. Queuing is used as a boundary between the
/// network layer (input/forward path) running with `NET_LOCK_SHARED()` and the transport
/// layer needing it exclusively.
pub fn ip_ours(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    _nxt: i32,
    af: i32,
    ns: Option<&Netstack>,
) -> i32 {
    // The caller's next header is recomputed from the (reassembled) packet.
    let nxt = ip_fragcheck(mp, offp);
    if nxt == IPPROTO_DONE {
        return IPPROTO_DONE;
    }

    // We are already in a IPv4/IPv6 local deliver loop.
    if af != i32::from(AF_UNSPEC) {
        return nxt;
    }

    let nxt = ip_deliver(mp, offp, nxt, i32::from(AF_INET), true, ns);
    if nxt == IPPROTO_DONE {
        return IPPROTO_DONE;
    }

    ip_ours_enqueue(mp, offp, nxt)
}

/// `ip_ours_enqueue`: queues the packet on `ipintrq` for `ipintr`, with its offset and next
/// protocol in a tag when the header has options.
pub fn ip_ours_enqueue(mp: &mut Option<&'static Mbuf>, offp: &mut i32, nxt: i32) -> i32 {
    let Some(m) = *mp else {
        return IPPROTO_DONE;
    };

    // save values for later, use after dequeue
    if *offp != size_of::<Ip>() as i32 {
        // mbuf tags are expensive, but only used for header options
        let Some(mtag) = m_tag_get(PACKET_TAG_IP_OFFNXT, size_of::<Ipoffnxt>() as i32, M_NOWAIT)
        else {
            ipstat_inc(IpstatCounters::IpsIdropped);
            m_freemp(mp);
            return IPPROTO_DONE;
        };
        let ion = Ipoffnxt {
            ion_off: *offp,
            ion_nxt: nxt,
        };
        // SAFETY: the tag has `size_of::<Ipoffnxt>()` bytes of data.
        unsafe { ptr::write_unaligned(mtag.data().cast::<Ipoffnxt>(), ion) };

        m_tag_prepend(m, mtag);
    }

    niq_enqueue(&IPINTRQ, m);
    *mp = None;
    IPPROTO_DONE
}

/// `ipintr`: dequeue and process locally delivered packets. This is called with exclusive
/// `NET_LOCK()`.
pub fn ipintr() {
    while let Some(m) = niq_dequeue(&IPINTRQ) {
        #[cfg(feature = "diagnostic")]
        if m.m_flags().get() & M_PKTHDR == 0 {
            panic(format_args!("ipintr no HDR"));
        }
        let (mut off, nxt);
        if let Some(mtag) = m_tag_find(m, PACKET_TAG_IP_OFFNXT, None) {
            // SAFETY: `ip_ours_enqueue` wrote an `Ipoffnxt` into the tag's data.
            let ion = unsafe { ptr::read_unaligned(mtag.data().cast::<Ipoffnxt>()) };
            off = ion.ion_off;
            nxt = ion.ion_nxt;

            // SAFETY: the tag is on this packet's list.
            unsafe { m_tag_delete(m, mtag) };
        } else {
            let ip = mtod_ip(m);
            off = i32::from(ip.ip_hl()) << 2;
            nxt = i32::from(ip.ip_p);
        }

        let mut mp = Some(m);
        let nxt = ip_deliver(&mut mp, &mut off, nxt, i32::from(AF_INET), false, None);
        kassert!(nxt == IPPROTO_DONE);
        let _ = nxt;
    }
}

/// `ipv4_input`: IPv4 input routine. Checksum and byte swap header. Process options. Forward
/// or deliver.
pub fn ipv4_input(ifp: &'static Ifnet, m: &'static Mbuf, ns: Option<&Netstack>) {
    let mut off = 0;
    let mut mp = Some(m);
    let nxt = ip_input_if(
        &mut mp,
        &mut off,
        IPPROTO_IPV4,
        i32::from(AF_UNSPEC),
        ifp,
        ns,
    );
    kassert!(nxt == IPPROTO_DONE);
    let _ = nxt;
}

/// `ipv4_check`: the header checks of `ipv4_input`; the packet trimmed to `ip_len`, or
/// `None` (freed) when it is bad.
pub fn ipv4_check(ifp: &Ifnet, m: &'static Mbuf) -> Option<&'static Mbuf> {
    let mut m = m;

    if (m.m_len().get() as usize) < size_of::<Ip>() {
        let Some(mm) = m_pullup(m, size_of::<Ip>() as i32) else {
            ipstat_inc(IpstatCounters::IpsToosmall);
            return None;
        };
        m = mm;
    }

    'bad: {
        let ip = mtod_ip(m);
        if ip.ip_v() != IPVERSION {
            ipstat_inc(IpstatCounters::IpsBadvers);
            break 'bad;
        }

        let hlen = usize::from(ip.ip_hl()) << 2;
        if hlen < size_of::<Ip>() {
            // minimum header length
            ipstat_inc(IpstatCounters::IpsBadhlen);
            break 'bad;
        }
        if hlen > m.m_len().get() as usize {
            let Some(mm) = m_pullup(m, hlen as i32) else {
                ipstat_inc(IpstatCounters::IpsBadhlen);
                return None;
            };
            m = mm;
        }

        // 127/8 must not appear on wire - RFC1122
        if ((ntohl(ip.ip_dst.s_addr) >> IN_CLASSA_NSHIFT) == IN_LOOPBACKNET
            || (ntohl(ip.ip_src.s_addr) >> IN_CLASSA_NSHIFT) == IN_LOOPBACKNET)
            && ifp.if_flags.get() & IFF_LOOPBACK == 0
        {
            ipstat_inc(IpstatCounters::IpsBadaddr);
            break 'bad;
        }

        let csum_flags = m.m_pkthdr().csum_flags.get();
        if csum_flags & M_IPV4_CSUM_IN_OK == 0 {
            if csum_flags & M_IPV4_CSUM_IN_BAD != 0 {
                ipstat_inc(IpstatCounters::IpsBadsum);
                break 'bad;
            }

            ipstat_inc(IpstatCounters::IpsInswcsum);
            if in_cksum(m, hlen as i32) != 0 {
                ipstat_inc(IpstatCounters::IpsBadsum);
                break 'bad;
            }

            m.m_pkthdr()
                .csum_flags
                .set(m.m_pkthdr().csum_flags.get() | M_IPV4_CSUM_IN_OK);
        }

        // Retrieve the packet length.
        let len = usize::from(ntohs(ip.ip_len));

        // Convert fields to host representation.
        if len < hlen {
            ipstat_inc(IpstatCounters::IpsBadlen);
            break 'bad;
        }

        // Check that the amount of data in the buffers is at least as much as the IP header
        // would have us expect. Trim mbufs if longer than we expect. Drop packet if shorter
        // than we expect.
        let pktlen = m.m_pkthdr().len.get() as usize;
        if pktlen < len {
            ipstat_inc(IpstatCounters::IpsTooshort);
            break 'bad;
        }
        if pktlen > len {
            if m.m_len().get() as usize == pktlen {
                m.m_len().set(len as u32);
                m.m_pkthdr().len.set(len as i32);
            } else {
                m_adj(m, len as i32 - pktlen as i32);
            }
        }

        return Some(m);
    }
    // bad:
    m_freem(m);
    None
}

/// `ip_input_if`: the IPv4 input of a packet received on `ifp`.
pub fn ip_input_if(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    nxt: i32,
    af: i32,
    ifp: &'static Ifnet,
    ns: Option<&Netstack>,
) -> i32 {
    let iproute = Route::new();
    let mut flags = 0;
    let mut nxt = nxt;

    kassert!(*offp == 0);

    ipstat_inc(IpstatCounters::IpsTotal);
    let ro: &Route = match ns {
        None => &iproute,
        Some(ns) => &ns.ns_route,
    };
    'out: {
        'bad: {
            let Some(m0) = *mp else {
                break 'bad;
            };
            *mp = ipv4_check(ifp, m0);
            let Some(m) = *mp else {
                break 'bad;
            };

            let ip = mtod_ip(m);

            // NCARP > 0: carp_lsdrop; not configured.

            // NPF > 0: the packet filter (pf_test, PF_IN), which may redirect it
            // (IP_REDIRECT); not configured.

            match ip_forwarding.load(Ordering::Relaxed) {
                2 => flags |= IP_FORWARDING_IPSEC | IP_FORWARDING,
                1 => flags |= IP_FORWARDING,
                _ => {}
            }
            if IP_DIRECTEDBCAST.load(Ordering::Relaxed) != 0 {
                flags |= IP_ALLOWBROADCAST;
            }

            let hlen = usize::from(ip.ip_hl()) << 2;

            // Process options and, if not destined for us, ship it on. ip_dooptions returns
            // 1 when an error was detected (causing an icmp message to be sent and the
            // original packet to be freed).
            if hlen > size_of::<Ip>() && ip_dooptions(m, ifp, flags) {
                *mp = None;
                break 'bad;
            }

            // `iproute` or `ns->ns_route`: chosen above.
            match in_ouraddr(m, ifp, ro, flags) {
                2 => break 'bad,
                1 => {
                    nxt = ip_ours(mp, offp, nxt, af, ns);
                    break 'out;
                }
                _ => {}
            }

            let ip = mtod_ip(m);
            if in_multicast(ip.ip_dst.s_addr) {
                // Make sure M_MCAST is set. It should theoretically already be there, but
                // let's play safe because upper layers check for this flag.
                m.m_flags().set(m.m_flags().get() | M_MCAST);

                // MROUTING: multicast forwarding through ip_mforward when
                // ip_mrouter_active; not configured.

                // See if we belong to the destination multicast group on the arrival
                // interface.
                if !in_hasmulti(&ip.ip_dst, ifp) {
                    ipstat_inc(IpstatCounters::IpsNotmember);
                    if !in_local_group(ip.ip_dst.s_addr) {
                        ipstat_inc(IpstatCounters::IpsCantforward);
                    }
                    break 'bad;
                }
                nxt = ip_ours(mp, offp, nxt, af, ns);
                break 'out;
            }

            // NCARP > 0: carp_lsdrop for ICMP; not configured.

            // Not for us; forward if possible and desirable.
            if flags & IP_FORWARDING == 0 {
                ipstat_inc(IpstatCounters::IpsCantforward);
                break 'bad;
            }
            // IPSEC: ipsec_forward_check when ipsec_in_use; not configured.

            ip_forward(m, ifp, Some(ro), flags);
            *mp = None;
            if ptr::eq(ro, &iproute) {
                rtfree(ro.ro_rt.get());
            }
            return IPPROTO_DONE;
        }
        // bad:
        nxt = IPPROTO_DONE;
        m_freemp(mp);
    }
    // out:
    if ptr::eq(ro, &iproute) {
        rtfree(ro.ro_rt.get());
    }
    nxt
}

/// `ip_fragcheck`: reassembles a fragment (the datagram, once complete, replaces `*mp`);
/// returns the next protocol with `*offp` at its header, or `IPPROTO_DONE`.
pub fn ip_fragcheck(mp: &mut Option<&'static Mbuf>, offp: &mut i32) -> i32 {
    let Some(mut m) = *mp else {
        return IPPROTO_DONE;
    };
    let mut ip = mtod_ip(m);
    let mut hlen = usize::from(ip.ip_hl()) << 2;

    // If offset or more fragments are set, must reassemble. Otherwise, nothing need be done.
    // (We could look in the reassembly queue to see if the packet was previously fragmented,
    // but it's not worth the time; just let them time out.)
    if ip.ip_off & htons(IP_OFFMASK | IP_MF) != 0 {
        if m.m_flags().get() & M_EXT != 0 {
            // XXX
            let Some(mm) = m_pullup(m, hlen as i32) else {
                *mp = None;
                ipstat_inc(IpstatCounters::IpsToosmall);
                return IPPROTO_DONE;
            };
            m = mm;
            *mp = Some(m);
            ip = mtod_ip(m);
        }

        // Adjust ip_len to not reflect header, set ipqe_mff if more fragments are expected,
        // convert offset of this to bytes.
        ip.ip_len = htons(ntohs(ip.ip_len) - hlen as u16);
        let mff = ip.ip_off & htons(IP_MF) != 0;
        if mff {
            // Make sure that fragments have a data length that's a non-zero multiple of 8
            // bytes.
            if ntohs(ip.ip_len) == 0 || ntohs(ip.ip_len) & 0x7 != 0 {
                mtod_ip_store(m, &ip);
                ipstat_inc(IpstatCounters::IpsBadfrags);
                m_freemp(mp);
                return IPPROTO_DONE;
            }
        }
        ip.ip_off = htons(ntohs(ip.ip_off) << 3);
        mtod_ip_store(m, &ip);

        mtx_enter(&IPQ_MUTEX);

        'bad: {
            // Look for queue of fragments of this datagram.
            let rdomain = rtable_l2(m.m_pkthdr().ph_rtableid.get());
            let fp = IPQ.0.iter().find(|fp| {
                ip.ip_id == fp.ipq_id.get()
                    && ip.ip_src.s_addr == fp.ipq_src.get().s_addr
                    && ip.ip_dst.s_addr == fp.ipq_dst.get().s_addr
                    && ip.ip_p == fp.ipq_p.get()
                    && rdomain == fp.ipq_rdomain.get()
            });
            // SAFETY: a queue on `ipq` lives until `ip_freef`/`ip_reass` free it, under
            // `ipq_mutex`, which is held.
            let fp: Option<&'static Ipq> = fp.map(|f| unsafe { &*ptr::from_ref(f) });

            // If datagram marked as having more fragments or if this is not the first
            // fragment, attempt reassembly; if it succeeds, proceed.
            if mff || ip.ip_off != 0 {
                let ip_maxqueue_local = IP_MAXQUEUE.load(Ordering::Relaxed);

                ipstat_inc(IpstatCounters::IpsFragments);
                if IP_FRAGS.load(Ordering::Relaxed) + 1 > ip_maxqueue_local {
                    ip_flush(ip_maxqueue_local);
                    ipstat_inc(IpstatCounters::IpsRcvmemdrop);
                    break 'bad;
                }

                let Some(mem) = pool_get(&IPQENT_POOL, PR_NOWAIT) else {
                    ipstat_inc(IpstatCounters::IpsRcvmemdrop);
                    break 'bad;
                };
                let qp = mem.as_ptr().cast::<Ipqent>();
                // SAFETY: a fresh pool item of `size_of::<Ipqent>()` bytes, written whole; it
                // lives until the reassembly frees it.
                let ipqe: &'static Ipqent = unsafe {
                    qp.write(Ipqent {
                        ipqe_q: crate::sys::queue::ListEntry::new(),
                        ipqe_ip: core::cell::Cell::new(mtod::<Ip>(m)),
                        ipqe_m: core::cell::Cell::new(Some(m)),
                        ipqe_mff: core::cell::Cell::new(u16::from(mff)),
                    });
                    &*qp
                };
                IP_FRAGS.fetch_add(1, Ordering::Relaxed);
                *mp = ip_reass(ipqe, fp, rdomain);
                let Some(mm) = *mp else {
                    break 'bad;
                };
                ipstat_inc(IpstatCounters::IpsReassembled);
                ip = mtod_ip(mm);
                hlen = usize::from(ip.ip_hl()) << 2;
                ip.ip_len = htons(ntohs(ip.ip_len) + hlen as u16);
                mtod_ip_store(mm, &ip);
            } else if let Some(fp) = fp {
                ip_freef(fp);
            }

            mtx_leave(&IPQ_MUTEX);
            *offp = hlen as i32;
            return i32::from(ip.ip_p);
        }
        // bad:
        mtx_leave(&IPQ_MUTEX);
        m_freemp(mp);
        return IPPROTO_DONE;
    }

    *offp = hlen as i32;
    i32::from(ip.ip_p)
}

/// `ip_deliver`: hands the packet to the protocols, walking the protocol switch until one
/// consumes it. With `shared` (the shared net lock is held), a protocol that needs the
/// exclusive lock gets the packet through `ipintrq` instead.
pub fn ip_deliver(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    nxt: i32,
    af: i32,
    shared: bool,
    ns: Option<&Netstack>,
) -> i32 {
    let mut nxt = nxt;
    let mut af = af;
    // INET6: the nesting counter of the IPv6 header chain; not configured.

    // Tell launch routine the next header
    ipstat_inc(IpstatCounters::IpsDelivered);

    while nxt != IPPROTO_DONE {
        let psw: &Protosw = match af {
            x if x == i32::from(AF_INET) => {
                &INETSW[usize::from(IP_PROTOX[nxt as usize].load(Ordering::Relaxed))]
            }
            // INET6: &inet6sw[ip6_protox[nxt]]; not configured.
            _ => panic(format_args!("ip_deliver: af {af}")),
        };
        if shared && psw.pr_flags & PR_MPINPUT == 0 {
            // delivery not finished, decrement counter, queue
            ipstat_dec(IpstatCounters::IpsDelivered);
            return ip_ours_enqueue(mp, offp, nxt);
        }

        // protection against faulty packet - there should be more sanity checks in header
        // chain processing.
        let Some(m) = *mp else {
            return IPPROTO_DONE;
        };
        if m.m_pkthdr().len.get() < *offp {
            ipstat_inc(IpstatCounters::IpsTooshort);
            m_freemp(mp);
            return IPPROTO_DONE;
        }

        // IPSEC: ipsec_local_check when ipsec_in_use; not configured.

        let naf = match nxt {
            IPPROTO_IPV4 => {
                ipstat_inc(IpstatCounters::IpsDelivered);
                i32::from(AF_INET)
            }
            // INET6: IPPROTO_IPV6 -> AF_INET6; not configured.
            _ => af,
        };
        let Some(input) = psw.pr_input else {
            panic(format_args!("ip_deliver: protocol {nxt} without input"));
        };
        nxt = input(mp, offp, nxt, af, ns);
        af = naf;
    }
    nxt
}

/// `in_ouraddr`: 1 if the packet is for one of our addresses (or a broadcast we accept), 2
/// if it was received on the wrong interface, 0 otherwise.
pub fn in_ouraddr(m: &'static Mbuf, ifp: &Ifnet, ro: &Route, flags: i32) -> i32 {
    let mut match_ = 0;

    // NPF > 0: pf_ouraddr; not configured.

    let ip = mtod_ip(m);

    if ip.ip_dst.s_addr == INADDR_BROADCAST || ip.ip_dst.s_addr == INADDR_ANY {
        m.m_flags().set(m.m_flags().get() | M_BCAST);
        return 1;
    }

    let rt = route_mpath(
        ro,
        &ip.ip_dst,
        Some(&ip.ip_src),
        m.m_pkthdr().ph_rtableid.get(),
    );
    if let Some(rt) = rt {
        if rt.rt_flags.get() & RTF_LOCAL != 0 {
            match_ = 1;
        }

        // If directedbcast is enabled we only consider it local if it is received on the
        // interface with that address.
        if rt.rt_flags.get() & RTF_BROADCAST != 0
            && (flags & IP_ALLOWBROADCAST == 0 || rt.rt_ifidx.get() == ifp.if_index.get())
        {
            match_ = 1;

            // Make sure M_BCAST is set
            m.m_flags().set(m.m_flags().get() | M_BCAST);
        }
    }

    if match_ == 0 {
        // No local address or broadcast address found, so check for ancient classful
        // broadcast addresses. It must have been broadcast on the link layer, and for an
        // address on the interface it was received on.
        if m.m_flags().get() & M_BCAST == 0
            || !in_classfulbroadcast(ip.ip_dst.s_addr, ip.ip_dst.s_addr)
        {
            return 0;
        }

        if ifp.if_rdomain.get() != rtable_l2(m.m_pkthdr().ph_rtableid.get()) {
            return 0;
        }
        // The check in the loop assumes you only rx a packet on an UP interface, and that
        // M_BCAST will only be set on a BROADCAST interface.
        net_assert_locked("in_ouraddr");
        for ifa in ifp.if_addrlist.iter() {
            // SAFETY: an interface address's `ifa_addr` is readable.
            if unsafe { (*ifa.ifa_addr.get()).sa_family } != AF_INET {
                continue;
            }

            if in_classfulbroadcast(ip.ip_dst.s_addr, ifatoia(ifa).ia_addr.get().sin_addr.s_addr) {
                match_ = 1;
                break;
            }
        }
    } else if let Some(rt) = rt
        && flags & IP_FORWARDING == 0
        && rt.rt_ifidx.get() != ifp.if_index.get()
        && !(ifp.if_flags.get() & IFF_LOOPBACK != 0
            || ifp.if_type.get() == IFT_ENC
            || m.m_pkthdr().pf.flags.get() & PF_TAG_TRANSLATE_LOCALHOST != 0)
    {
        // received on wrong interface.
        // NCARP > 0: carp_strict_addr_chk of the outgoing interface; not configured.
        ipstat_inc(IpstatCounters::IpsWrongif);
        match_ = 2;
    }

    match_
}

/// `ip_reass`: takes incoming datagram fragment `ipqe` and tries to reassemble it into a
/// whole datagram. If a chain for reassembly of this datagram already exists, it is given as
/// `fp`; otherwise one is made. Returns the datagram when complete.
pub fn ip_reass(
    ipqe: &'static Ipqent,
    fp: Option<&'static Ipq>,
    rdomain: u32,
) -> Option<&'static Mbuf> {
    let Some(m) = ipqe.ipqe_m.get() else {
        panic(format_args!("ip_reass: fragment without packet"));
    };
    let hlen = u32::from(ipqe_ip(ipqe).ip_hl()) << 2;

    crate::sys::mutex::mutex_assert_locked(&IPQ_MUTEX, "ip_reass");

    // Presence of header sizes in mbufs would confuse code below.
    m.m_data().set(m.m_data().get().wrapping_add(hlen as usize));
    m.m_len().set(m.m_len().get() - hlen);

    let dropfrag = |m: &'static Mbuf| -> Option<&'static Mbuf> {
        ipstat_inc(IpstatCounters::IpsFragdropped);
        m_freem(m);
        ipqent_put(ipqe);
        IP_FRAGS.fetch_sub(1, Ordering::Relaxed);
        None
    };

    let mut p: Option<&'static Ipqent> = None;
    let fp: &'static Ipq = match fp {
        None => {
            // If first fragment to arrive, create a reassembly queue.
            let Some(mem) = pool_get(&IPQ_POOL, PR_NOWAIT) else {
                return dropfrag(m);
            };
            let q = mem.as_ptr().cast::<Ipq>();
            let ip = ipqe_ip(ipqe);
            // SAFETY: a fresh pool item of `size_of::<Ipq>()` bytes, written whole; it lives
            // until `ip_freef` or the end of the reassembly.
            let fp: &'static Ipq = unsafe {
                q.write(Ipq {
                    ipq_q: crate::sys::queue::ListEntry::new(),
                    ipq_rdomain: core::cell::Cell::new(rdomain),
                    ipq_id: core::cell::Cell::new(ip.ip_id),
                    ipq_ttl: core::cell::Cell::new(IPFRAGTTL),
                    ipq_p: core::cell::Cell::new(ip.ip_p),
                    ipq_fragq: ListHead::new(),
                    ipq_src: core::cell::Cell::new(ip.ip_src),
                    ipq_dst: core::cell::Cell::new(ip.ip_dst),
                });
                &*q
            };
            // SAFETY: `fp` is on no list; `ipq` is changed under `ipq_mutex`.
            unsafe { IPQ.0.insert_head(fp) };
            // goto insert
            return ip_reass_insert(fp, ipqe, None);
        }
        Some(fp) => fp,
    };

    // Handle ECN by comparing this segment with the first one; if CE is set, do not lose
    // CE. drop if CE and not-ECT are mixed for the same packet.
    let Some(first) = fp.ipq_fragq.first() else {
        panic(format_args!("ip_reass: empty queue"));
    };
    // SAFETY: a fragment on the queue lives until freed under `ipq_mutex`.
    let first: &'static Ipqent = unsafe { &*ptr::from_ref(first) };
    let mut ip = ipqe_ip(ipqe);
    let ecn = ip.ip_tos & IPTOS_ECN_MASK;
    let mut ip0 = ipqe_ip(first);
    let ecn0 = ip0.ip_tos & IPTOS_ECN_MASK;
    if ecn == IPTOS_ECN_CE {
        if ecn0 == IPTOS_ECN_NOTECT {
            return dropfrag(m);
        }
        if ecn0 != IPTOS_ECN_CE {
            ip0.ip_tos |= IPTOS_ECN_CE;
            ipqe_ip_store(first, &ip0);
        }
    }
    if ecn == IPTOS_ECN_NOTECT && ecn0 != IPTOS_ECN_NOTECT {
        return dropfrag(m);
    }

    // Find a segment which begins after this one does.
    let mut q = fp.ipq_fragq.first().map(|e| e as *const Ipqent);
    while let Some(qq) = q {
        // SAFETY: as above.
        let qe: &'static Ipqent = unsafe { &*qq };
        if ntohs(ipqe_ip(qe).ip_off) > ntohs(ip.ip_off) {
            break;
        }
        p = Some(qe);
        q = ListHead::<crate::netinet::ip_var::Ipqehead>::next(qe).map(|e| e as *const Ipqent);
    }

    // If there is a preceding segment, it may provide some of our data already. If so, drop
    // the data from the incoming segment. If it provides all of our data, drop us.
    if let Some(pe) = p {
        let pip = ipqe_ip(pe);
        let i = i32::from(ntohs(pip.ip_off)) + i32::from(ntohs(pip.ip_len))
            - i32::from(ntohs(ip.ip_off));
        if i > 0 {
            if i >= i32::from(ntohs(ip.ip_len)) {
                return dropfrag(m);
            }
            m_adj(m, i);
            ip.ip_off = htons(ntohs(ip.ip_off) + i as u16);
            ip.ip_len = htons(ntohs(ip.ip_len) - i as u16);
            ipqe_ip_store(ipqe, &ip);
        }
    }

    // While we overlap succeeding segments trim them or, if they are completely covered,
    // dequeue them.
    while let Some(qq) = q {
        // SAFETY: as above.
        let qe: &'static Ipqent = unsafe { &*qq };
        let mut qip = ipqe_ip(qe);
        if i32::from(ntohs(ip.ip_off)) + i32::from(ntohs(ip.ip_len)) <= i32::from(ntohs(qip.ip_off))
        {
            break;
        }
        let i = (i32::from(ntohs(ip.ip_off)) + i32::from(ntohs(ip.ip_len)))
            - i32::from(ntohs(qip.ip_off));
        if i < i32::from(ntohs(qip.ip_len)) {
            qip.ip_len = htons(ntohs(qip.ip_len) - i as u16);
            qip.ip_off = htons(ntohs(qip.ip_off) + i as u16);
            ipqe_ip_store(qe, &qip);
            m_adj(qe.ipqe_m.get(), i);
            break;
        }
        let nq = ListHead::<crate::netinet::ip_var::Ipqehead>::next(qe).map(|e| e as *const Ipqent);
        m_freem(qe.ipqe_m.get());
        // SAFETY: `qe` is on this queue, under `ipq_mutex`.
        unsafe { ListHead::<crate::netinet::ip_var::Ipqehead>::remove(qe) };
        ipqent_put(qe);
        IP_FRAGS.fetch_sub(1, Ordering::Relaxed);
        q = nq;
    }

    ip_reass_insert(fp, ipqe, p)
}

/// The `insert:` part of `ip_reass`: sticks the new segment in its place and checks for
/// complete reassembly.
fn ip_reass_insert(
    fp: &'static Ipq,
    ipqe: &'static Ipqent,
    p: Option<&'static Ipqent>,
) -> Option<&'static Mbuf> {
    // SAFETY: `ipqe` is on no list; the queue changes under `ipq_mutex`.
    unsafe {
        match p {
            None => fp.ipq_fragq.insert_head(ipqe),
            Some(p) => ListHead::<crate::netinet::ip_var::Ipqehead>::insert_after(p, ipqe),
        }
    }
    let mut next: i32 = 0;
    let mut last: Option<&Ipqent> = None;
    for q in fp.ipq_fragq.iter() {
        let qip = ipqe_ip(q);
        if i32::from(ntohs(qip.ip_off)) != next {
            return None;
        }
        next += i32::from(ntohs(qip.ip_len));
        last = Some(q);
    }
    if last.is_some_and(|l| l.ipqe_mff.get() != 0) {
        return None;
    }

    // Reassembly is complete. Check for a bogus message size and concatenate fragments.
    let q = fp.ipq_fragq.first()?;
    // SAFETY: a fragment on the queue lives until freed under `ipq_mutex`.
    let q: &'static Ipqent = unsafe { &*ptr::from_ref(q) };
    let mut ip = ipqe_ip(q);
    let iphdr = q.ipqe_ip.get();
    if next as usize + (usize::from(ip.ip_hl()) << 2) > IP_MAXPACKET {
        ipstat_inc(IpstatCounters::IpsToolong);
        ip_freef(fp);
        return None;
    }
    let Some(m) = q.ipqe_m.get() else {
        panic(format_args!("ip_reass: fragment without packet"));
    };
    let t = m.m_next().get();
    m.m_next().set(None);
    m_cat(m, t);
    let mut nq = ListHead::<crate::netinet::ip_var::Ipqehead>::next(q).map(|e| e as *const Ipqent);
    ipqent_put(q);
    IP_FRAGS.fetch_sub(1, Ordering::Relaxed);
    while let Some(qq) = nq {
        // SAFETY: as above.
        let qe: &'static Ipqent = unsafe { &*qq };
        let t = qe.ipqe_m.get();
        nq = ListHead::<crate::netinet::ip_var::Ipqehead>::next(qe).map(|e| e as *const Ipqent);
        ipqent_put(qe);
        IP_FRAGS.fetch_sub(1, Ordering::Relaxed);
        if let Some(t) = t {
            m_removehdr(t);
        }
        m_cat(m, t);
    }

    // Create header for new ip packet by modifying header of first packet; dequeue and
    // discard fragment reassembly header. Make header visible.
    ip.ip_len = htons(next as u16);
    ip.ip_src = fp.ipq_src.get();
    ip.ip_dst = fp.ipq_dst.get();
    // SAFETY: `iphdr` is the first fragment's header, in front of its (now trimmed) data.
    unsafe { ptr::write_unaligned(iphdr, ip) };
    // SAFETY: `fp` is on `ipq`, under `ipq_mutex`.
    unsafe { ListHead::<IpqList>::remove(fp) };
    pool_put(&IPQ_POOL, ptr::NonNull::from(fp).cast());
    let hl = u32::from(ip.ip_hl()) << 2;
    m.m_len().set(m.m_len().get() + hl);
    m.m_data().set(m.m_data().get().wrapping_sub(hl as usize));
    m_calchdrlen(m);
    Some(m)
}

/// The IP header of a queued fragment, as a value.
fn ipqe_ip(ipqe: &Ipqent) -> Ip {
    // SAFETY: `ipqe_ip` points at the fragment's header, which the entry's mbuf holds.
    unsafe { ptr::read_unaligned(ipqe.ipqe_ip.get()) }
}

/// Writes back the IP header of a queued fragment.
fn ipqe_ip_store(ipqe: &Ipqent, ip: &Ip) {
    // SAFETY: as in `ipqe_ip`.
    unsafe { ptr::write_unaligned(ipqe.ipqe_ip.get(), *ip) };
}

/// Frees a reassembly entry.
fn ipqent_put(ipqe: &Ipqent) {
    pool_put(&IPQENT_POOL, ptr::NonNull::from(ipqe).cast());
}

/// `ip_freef`: frees a fragment reassembly header and all associated datagrams.
pub fn ip_freef(fp: &'static Ipq) {
    crate::sys::mutex::mutex_assert_locked(&IPQ_MUTEX, "ip_freef");

    while let Some(q) = fp.ipq_fragq.first() {
        // SAFETY: `q` is on this queue, under `ipq_mutex`.
        unsafe { ListHead::<crate::netinet::ip_var::Ipqehead>::remove(q) };
        m_freem(q.ipqe_m.get());
        ipqent_put(q);
        IP_FRAGS.fetch_sub(1, Ordering::Relaxed);
    }
    // SAFETY: `fp` is on `ipq`, under `ipq_mutex`.
    unsafe { ListHead::<IpqList>::remove(fp) };
    pool_put(&IPQ_POOL, ptr::NonNull::from(fp).cast());
}

/// `ip_slowtimo`: IP timer processing; if a timer expires on a reassembly queue, discard it.
pub fn ip_slowtimo() {
    mtx_enter(&IPQ_MUTEX);
    for fp in IPQ.0.iter() {
        let ttl = fp.ipq_ttl.get().wrapping_sub(1);
        fp.ipq_ttl.set(ttl);
        if ttl == 0 {
            ipstat_inc(IpstatCounters::IpsFragtimeout);
            // SAFETY: on the list, under `ipq_mutex`; the iterator already read the next.
            ip_freef(unsafe { &*ptr::from_ref(fp) });
        }
    }
    mtx_leave(&IPQ_MUTEX);
}

/// `ip_flush`: flush a bunch of datagram fragments, till we are down to 75%.
fn ip_flush(maxqueue: i32) {
    let mut max = 50;

    crate::sys::mutex::mutex_assert_locked(&IPQ_MUTEX, "ip_flush");

    while let Some(fp) = IPQ.0.first() {
        if IP_FRAGS.load(Ordering::Relaxed) <= maxqueue * 3 / 4 {
            break;
        }
        max -= 1;
        if max == 0 {
            break;
        }
        ipstat_inc(IpstatCounters::IpsFragdropped);
        // SAFETY: on the list, under `ipq_mutex`.
        ip_freef(unsafe { &*ptr::from_ref(fp) });
    }
}

/// `ip_dooptions`: do option processing on a datagram, possibly discarding it if bad options
/// are encountered, or forwarding it if source-routed. Returns `true` if the packet has been
/// forwarded/freed, `false` if it should be processed further.
pub fn ip_dooptions(m: &'static Mbuf, ifp: &'static Ifnet, flags: i32) -> bool {
    let mut ip = mtod_ip(m);
    let rtableid = m.m_pkthdr().ph_rtableid.get();
    let mut type_ = ICMP_PARAMPROB;
    let mut code: i32 = 0;
    let mut forward = false;

    let dst = ip.ip_dst;
    let hdr = size_of::<Ip>();
    let cnt0 = (usize::from(ip.ip_hl()) << 2) - hdr;
    // SAFETY: `ipv4_check` made the whole header (with its options) contiguous in the first
    // mbuf; the options follow the fixed header.
    let opts: &mut [u8] = unsafe { slice::from_raw_parts_mut(mtod::<u8>(m).add(hdr), cnt0) };

    // KERNEL_LOCK(): no kernel lock without MULTIPROCESSOR.
    let bad = 'bad: {
        let mut cp = 0usize;
        let mut cnt = cnt0 as i32;
        'opts: while cnt > 0 {
            let opt = opts[cp + IPOPT_OPTVAL];
            if opt == IPOPT_EOL {
                break;
            }
            let optlen: i32;
            if opt == IPOPT_NOP {
                optlen = 1;
            } else {
                if cnt < (IPOPT_OLEN + 1) as i32 {
                    code = (hdr + cp + IPOPT_OLEN) as i32;
                    break 'bad true;
                }
                optlen = i32::from(opts[cp + IPOPT_OLEN]);
                if optlen < (IPOPT_OLEN + 1) as i32 || optlen > cnt {
                    code = (hdr + cp + IPOPT_OLEN) as i32;
                    break 'bad true;
                }
            }

            match opt {
                // Source routing with record. Find interface with current destination address.
                // If none on this machine then drop if strictly routed, or do nothing if
                // loosely routed. Record interface address and bring up next address
                // component. If strictly routed make sure next address is on directly
                // accessible net.
                IPOPT_LSRR | IPOPT_SSRR => {
                    if IP_DOSOURCEROUTE.load(Ordering::Relaxed) == 0 {
                        type_ = ICMP_UNREACH;
                        code = i32::from(ICMP_UNREACH_SRCFAIL);
                        break 'bad true;
                    }
                    if optlen < (IPOPT_OFFSET + 1) as i32 {
                        code = (hdr + cp + IPOPT_OLEN) as i32;
                        break 'bad true;
                    }
                    let mut off = opts[cp + IPOPT_OFFSET];
                    if off < IPOPT_MINOFF {
                        code = (hdr + cp + IPOPT_OFFSET) as i32;
                        break 'bad true;
                    }
                    let mut ipaddr = SockaddrIn {
                        sin_family: AF_INET,
                        sin_len: size_of::<SockaddrIn>() as u8,
                        sin_addr: ip.ip_dst,
                        ..SockaddrIn::default()
                    };
                    // SAFETY: a local `sockaddr_in`.
                    let ia = unsafe { ifa_ifwithaddr(sintosa(&mut ipaddr), rtableid) };
                    if ia.is_none() {
                        if opt == IPOPT_SSRR {
                            type_ = ICMP_UNREACH;
                            code = i32::from(ICMP_UNREACH_SRCFAIL);
                            break 'bad true;
                        }
                        // Loose routing, and not at next destination yet; nothing to do
                        // except forward.
                    } else {
                        off -= 1; // 0 origin
                        let off = usize::from(off);
                        if off + size_of::<InAddr>() > optlen as usize {
                            // End of source route. Should be for us.
                            save_rte(m, &opts[cp..cp + optlen as usize], ip.ip_src);
                        } else {
                            // locate outgoing interface
                            let mut a = [0u8; 4];
                            a.copy_from_slice(&opts[cp + off..cp + off + 4]);
                            let mut ipaddr = SockaddrIn {
                                sin_family: AF_INET,
                                sin_len: size_of::<SockaddrIn>() as u8,
                                sin_addr: InAddr {
                                    s_addr: u32::from_ne_bytes(a),
                                },
                                ..SockaddrIn::default()
                            };
                            // keep packet in the virtual instance
                            // SAFETY: a local `sockaddr_in`.
                            let rt = unsafe { rtalloc(sintosa(&mut ipaddr), RT_RESOLVE, rtableid) };
                            let Some(r) = rt.filter(|r| {
                                rtisvalid(Some(r))
                                    && !(opt == IPOPT_SSRR && r.rt_flags.get() & RTF_GATEWAY != 0)
                            }) else {
                                type_ = ICMP_UNREACH;
                                code = i32::from(ICMP_UNREACH_SRCFAIL);
                                rtfree(rt);
                                break 'bad true;
                            };
                            let ia = ifatoia(r.ifa());
                            opts[cp + off..cp + off + 4]
                                .copy_from_slice(&ia.ia_addr.get().sin_addr.s_addr.to_ne_bytes());
                            rtfree(Some(r));
                            opts[cp + IPOPT_OFFSET] += size_of::<InAddr>() as u8;
                            ip.ip_dst = ipaddr.sin_addr;
                            mtod_ip_store(m, &ip);
                            // Let ip_intr's mcast routing check handle mcast pkts
                            forward = !in_multicast(ip.ip_dst.s_addr);
                        }
                    }
                }

                IPOPT_RR => 'rr: {
                    if optlen < (IPOPT_OFFSET + 1) as i32 {
                        code = (hdr + cp + IPOPT_OLEN) as i32;
                        break 'bad true;
                    }
                    let off = opts[cp + IPOPT_OFFSET];
                    if off < IPOPT_MINOFF {
                        code = (hdr + cp + IPOPT_OFFSET) as i32;
                        break 'bad true;
                    }

                    // If no space remains, ignore.
                    let off = usize::from(off - 1); // 0 origin
                    if off + size_of::<InAddr>() > optlen as usize {
                        break 'rr;
                    }
                    let mut ipaddr = SockaddrIn {
                        sin_family: AF_INET,
                        sin_len: size_of::<SockaddrIn>() as u8,
                        sin_addr: ip.ip_dst,
                        ..SockaddrIn::default()
                    };
                    // locate outgoing interface; if we're the destination, use the incoming
                    // interface (should be same). Again keep the packet inside the virtual
                    // instance.
                    // SAFETY: a local `sockaddr_in`.
                    let rt = unsafe { rtalloc(sintosa(&mut ipaddr), RT_RESOLVE, rtableid) };
                    let Some(r) = rt.filter(|r| rtisvalid(Some(r))) else {
                        type_ = ICMP_UNREACH;
                        code = i32::from(ICMP_UNREACH_HOST);
                        rtfree(rt);
                        break 'bad true;
                    };
                    let ia = ifatoia(r.ifa());
                    opts[cp + off..cp + off + 4]
                        .copy_from_slice(&ia.ia_addr.get().sin_addr.s_addr.to_ne_bytes());
                    rtfree(Some(r));
                    opts[cp + IPOPT_OFFSET] += size_of::<InAddr>() as u8;
                }

                IPOPT_TS => 'ts: {
                    code = (hdr + cp) as i32;
                    if (optlen as usize) < size_of::<IpTimestamp>() {
                        break 'bad true;
                    }
                    // SAFETY: the option holds at least an `ip_timestamp`, plain bytes.
                    let mut ipt: IpTimestamp =
                        unsafe { ptr::read_unaligned(opts[cp..].as_ptr().cast::<IpTimestamp>()) };
                    if ipt.ipt_ptr < 5 || ipt.ipt_len < 5 {
                        break 'bad true;
                    }
                    if usize::from(ipt.ipt_ptr) - 1 + size_of::<u32>() > usize::from(ipt.ipt_len) {
                        // The overflow count of the local copy, as the C's (it is not written
                        // back).
                        let oflw = (ipt.ipt_oflw() + 1) & 0x0f;
                        ipt.set_ipt_oflw(oflw);
                        if oflw == 0 {
                            break 'bad true;
                        }
                        break 'ts;
                    }
                    let p = cp + usize::from(ipt.ipt_ptr) - 1;
                    let mut sin = [0u8; 4];
                    sin.copy_from_slice(&opts[p..p + 4]);
                    let mut sin = InAddr {
                        s_addr: u32::from_ne_bytes(sin),
                    };
                    match ipt.ipt_flg() {
                        IPOPT_TS_TSONLY => {}

                        IPOPT_TS_TSANDADDR => {
                            if usize::from(ipt.ipt_ptr) - 1 + size_of::<u32>() + size_of::<InAddr>()
                                > usize::from(ipt.ipt_len)
                            {
                                break 'bad true;
                            }
                            let mut ipaddr = SockaddrIn {
                                sin_family: AF_INET,
                                sin_len: size_of::<SockaddrIn>() as u8,
                                sin_addr: dst,
                                ..SockaddrIn::default()
                            };
                            // SAFETY: a local `sockaddr_in`.
                            let Some(ia) = (unsafe { ifaof_ifpforaddr(sintosa(&mut ipaddr), ifp) })
                            else {
                                // continue: the next option.
                                cnt -= optlen;
                                cp += optlen as usize;
                                continue 'opts;
                            };
                            sin = ifatoia(ia).ia_addr.get().sin_addr;
                            ipt.ipt_ptr += size_of::<InAddr>() as u8;
                        }

                        IPOPT_TS_PRESPEC => {
                            if usize::from(ipt.ipt_ptr) - 1 + size_of::<u32>() + size_of::<InAddr>()
                                > usize::from(ipt.ipt_len)
                            {
                                break 'bad true;
                            }
                            let mut ipaddr = SockaddrIn {
                                sin_family: AF_INET,
                                sin_len: size_of::<SockaddrIn>() as u8,
                                sin_addr: sin,
                                ..SockaddrIn::default()
                            };
                            // SAFETY: a local `sockaddr_in`.
                            if unsafe { ifa_ifwithaddr(sintosa(&mut ipaddr), rtableid) }.is_none() {
                                cnt -= optlen;
                                cp += optlen as usize;
                                continue 'opts;
                            }
                            ipt.ipt_ptr += size_of::<InAddr>() as u8;
                        }

                        _ => {
                            code = (hdr + cp + IPOPT_OFFSET + 1) as i32;
                            break 'bad true;
                        }
                    }
                    let _ = sin;
                    let ntime = iptime();
                    let q = cp + usize::from(ipt.ipt_ptr) - 1;
                    if q + 4 <= opts.len() {
                        opts[q..q + 4].copy_from_slice(&ntime.to_ne_bytes());
                    }
                    ipt.ipt_ptr += size_of::<u32>() as u8;
                }

                _ => {}
            }
            cnt -= optlen;
            cp += optlen as usize;
        }
        false
    };
    // KERNEL_UNLOCK()
    if bad {
        icmp_error(m, type_, code as u8, 0, 0);
        ipstat_inc(IpstatCounters::IpsBadoptions);
        return true;
    }
    if forward && flags & IP_FORWARDING != 0 {
        ip_forward(m, ifp, None, flags | IP_REDIRECT);
        return true;
    }
    false
}

/// `save_rte`: saves incoming source route `option` for use in replies, to be picked up later
/// by `ip_srcroute` if the receiver is interested.
fn save_rte(m: &Mbuf, option: &[u8], dst: InAddr) {
    let olen = usize::from(option[IPOPT_OLEN]);
    let routes = MAX_IPOPTLEN / size_of::<InAddr>() * size_of::<InAddr>();
    if olen > IPOPT_OFFSET + 1 + routes {
        return;
    }

    let Some(mtag) = m_tag_get(PACKET_TAG_SRCROUTE, size_of::<IpSrcrt>() as i32, M_NOWAIT) else {
        ipstat_inc(IpstatCounters::IpsIdropped);
        return;
    };

    // The option's bytes go to isr_hdr and on into isr_routes, as the C's memcpy does.
    let mut raw = [0u8; IPOPT_OFFSET + 1 + MAX_IPOPTLEN];
    raw[..olen].copy_from_slice(&option[..olen]);
    let mut isr = IpSrcrt {
        isr_nhops: ((olen - IPOPT_OFFSET - 1) / size_of::<InAddr>()) as i32,
        isr_dst: dst,
        isr_nop: 0,
        isr_hdr: [0; IPOPT_OFFSET + 1],
        isr_routes: [InAddr::default(); MAX_IPOPTLEN / size_of::<InAddr>()],
    };
    isr.isr_hdr.copy_from_slice(&raw[..IPOPT_OFFSET + 1]);
    for (i, r) in isr.isr_routes.iter_mut().enumerate() {
        let o = IPOPT_OFFSET + 1 + i * 4;
        let mut a = [0u8; 4];
        a.copy_from_slice(&raw[o..o + 4]);
        r.s_addr = u32::from_ne_bytes(a);
    }
    // SAFETY: the tag has `size_of::<IpSrcrt>()` bytes of data.
    unsafe { ptr::write_unaligned(mtag.data().cast::<IpSrcrt>(), isr) };
    m_tag_prepend(m, mtag);
}

/// `ip_srcroute`: retrieves the incoming source route for use in replies, in the same form
/// used by setsockopt. The first hop is placed before the options, will be removed later.
pub fn ip_srcroute(m0: &Mbuf) -> Option<&'static Mbuf> {
    if IP_DOSOURCEROUTE.load(Ordering::Relaxed) == 0 {
        return None;
    }

    let mtag = m_tag_find(m0, PACKET_TAG_SRCROUTE, None)?;
    // SAFETY: `save_rte` wrote an `IpSrcrt` into the tag's data.
    let mut isr = unsafe { ptr::read_unaligned(mtag.data().cast::<IpSrcrt>()) };

    if isr.isr_nhops == 0 {
        return None;
    }
    let Some(m) = m_get(M_DONTWAIT, MT_SOOPTS) else {
        ipstat_inc(IpstatCounters::IpsIdropped);
        return None;
    };

    const OPTSIZ: usize = 1 + IPOPT_OFFSET + 1; // sizeof(isr_nop) + sizeof(isr_hdr)

    // length is (nhops+1)*sizeof(addr) + sizeof(nop + header)
    let nhops = isr.isr_nhops as usize;
    let len = (nhops + 1) * size_of::<InAddr>() + OPTSIZ;
    m.m_len().set(len as u32);

    let mut out = [0u8; MHLEN];
    // First save first hop for return route
    out[..4].copy_from_slice(&isr.isr_routes[nhops - 1].s_addr.to_ne_bytes());

    // Copy option fields and padding (nop) to mbuf.
    isr.isr_nop = crate::netinet::ip::IPOPT_NOP;
    isr.isr_hdr[IPOPT_OFFSET] = IPOPT_MINOFF;
    out[4] = isr.isr_nop;
    out[5..4 + OPTSIZ].copy_from_slice(&isr.isr_hdr);
    // Record return path as an IP source route, reversing the path (pointers are now
    // aligned).
    let mut q = 4 + OPTSIZ;
    for p in (0..nhops - 1).rev() {
        out[q..q + 4].copy_from_slice(&isr.isr_routes[p].s_addr.to_ne_bytes());
        q += 4;
    }
    // Last hop goes to final destination.
    out[q..q + 4].copy_from_slice(&isr.isr_dst.s_addr.to_ne_bytes());
    // SAFETY: a fresh mbuf has MLEN bytes at its data pointer, more than `len`.
    unsafe { ptr::copy_nonoverlapping(out.as_ptr(), mtod::<u8>(m), len) };
    // SAFETY: the tag is on `m0`'s list.
    unsafe { m_tag_delete(m0, mtag) };
    Some(m)
}

/// `ip_stripoptions`: strips out IP options, at higher level protocol in the kernel.
pub fn ip_stripoptions(m: &Mbuf) {
    let mut ip = mtod_ip(m);
    let olen = (usize::from(ip.ip_hl()) << 2) - size_of::<Ip>();
    let opts = mtod::<u8>(m).wrapping_add(size_of::<Ip>());
    let i = m.m_len().get() as usize - (size_of::<Ip>() + olen);
    // SAFETY: the first mbuf holds the header, its options and `i` more bytes.
    unsafe { ptr::copy(opts.add(olen), opts, i) };
    m.m_len().set(m.m_len().get() - olen as u32);
    if m.m_flags().get() & M_PKTHDR != 0 {
        m.m_pkthdr().len.set(m.m_pkthdr().len.get() - olen as i32);
    }
    ip.set_ip_hl((size_of::<Ip>() >> 2) as u8);
    ip.ip_len = htons(ntohs(ip.ip_len) - olen as u16);
    mtod_ip_store(m, &ip);
}

/// `ip_forward`: forwards a packet. If some error occurs return the sender an icmp packet.
/// Note we can't always generate a meaningful icmp message because icmp doesn't have a large
/// enough repertoire of codes and types. If not forwarding, just drop the packet. This could
/// be confusing if ip_forwarding was zero but some routing protocol was advancing us as a
/// gateway to somewhere. However, we must let the routing protocol deal with that.
pub fn ip_forward(m: &'static Mbuf, ifp: &Ifnet, ro: Option<&Route>, flags: i32) {
    let mut ip = mtod_ip(m);
    let iproute = Route::new();
    let rtableid = m.m_pkthdr().ph_rtableid.get();
    let loopcnt = m.m_pkthdr().ph_loopcnt.get();
    let mut icmp_buf = [0u8; 68];
    let mut type_ = 0u8;
    let mut code = 0u8;
    let mut destmtu = 0i32;
    let mut dest = 0u32;

    let ro = ro.unwrap_or(&iproute);
    'done: {
        if m.m_flags().get() & (M_BCAST | M_MCAST) != 0
            || !in_canforward(ip.ip_dst)
            || ip.ip_src.s_addr == INADDR_ANY
        {
            ipstat_inc(IpstatCounters::IpsCantforward);
            m_freem(m);
            break 'done;
        }
        if ip.ip_ttl <= IPTTLDEC {
            icmp_error(m, ICMP_TIMXCEED, ICMP_TIMXCEED_INTRANS, dest, 0);
            break 'done;
        }

        let Some(rt) = route_mpath(ro, &ip.ip_dst, Some(&ip.ip_src), rtableid) else {
            ipstat_inc(IpstatCounters::IpsNoroute);
            icmp_error(m, ICMP_UNREACH, ICMP_UNREACH_HOST, dest, 0);
            break 'done;
        };

        // Save at most 68 bytes of the packet in case we need to generate an ICMP message to
        // the src. The data is saved on the stack. A new mbuf is only allocated when ICMP is
        // actually created.
        let icmp_len = icmp_buf.len().min(usize::from(ntohs(ip.ip_len)));
        let mflags = m.m_flags().get();
        let pfflags = m.m_pkthdr().pf.flags.get();
        m_copydata(m, 0, &mut icmp_buf[..icmp_len]);

        ip.ip_ttl -= IPTTLDEC;
        mtod_ip_store(m, &ip);

        // If forwarding packet using same interface that it came in on, perhaps should send a
        // redirect to sender to shortcut a hop. Only send redirect if source is sending
        // directly to us, and if packet was not source routed (or has any options). Also,
        // don't send redirect if forwarding using a default route or a route modified by a
        // redirect. Don't send redirect if we advertise destination's arp address as ours
        // (proxy arp).
        // SAFETY: a route's key is a readable `sockaddr_in` in the inet table.
        let key = unsafe { (*crate::netinet::in_::satosin_const(rt_key(rt))).sin_addr };
        if rt.rt_ifidx.get() == ifp.if_index.get()
            && rt.rt_flags.get() & (RTF_DYNAMIC | RTF_MODIFIED) == 0
            && key.s_addr != INADDR_ANY
            && flags & IP_REDIRECT == 0
            // NETHER > 0
            && !arpproxy(key, rtableid)
            && IP_SENDREDIRECTS.load(Ordering::Relaxed) != 0
        {
            let ia = ifatoia(rt.ifa());
            if ip.ip_src.s_addr & ia.ia_netmask.get() == ia.ia_net.get() {
                if rt.rt_flags.get() & RTF_GATEWAY != 0 {
                    // SAFETY: a gateway route's gateway is a `sockaddr_in`.
                    dest = unsafe {
                        (*crate::netinet::in_::satosin_const(rt.rt_gateway.get()))
                            .sin_addr
                            .s_addr
                    };
                } else {
                    dest = ip.ip_dst.s_addr;
                }
                // Router requirements says to only send host redirects
                type_ = ICMP_REDIRECT;
                code = ICMP_REDIRECT_HOST;
            }
        }

        let error = ip_output(m, None, Some(ro), flags | IP_FORWARDING, None, 0);
        let rt = ro.ro_rt.get();
        if error.is_err() {
            ipstat_inc(IpstatCounters::IpsCantforward);
        } else {
            ipstat_inc(IpstatCounters::IpsForward);
            if type_ != 0 {
                ipstat_inc(IpstatCounters::IpsRedirectsent);
            } else {
                break 'done;
            }
        }
        match error {
            Ok(()) => {
                // forwarded, but need redirect: type, code set above
            }

            Err(Errno::EMSGSIZE) => {
                type_ = ICMP_UNREACH;
                code = ICMP_UNREACH_NEEDFRAG;
                if let Some(rt) = rt {
                    let rtmtu = rt.rt_mtu().load(Ordering::Relaxed);
                    if rtmtu != 0 {
                        destmtu = rtmtu as i32;
                    } else {
                        let destifp = if_get(rt.rt_ifidx.get());
                        if let Some(d) = destifp {
                            destmtu = d.if_mtu.get() as i32;
                        }
                        if_put(destifp);
                    }
                }
                ipstat_inc(IpstatCounters::IpsCantfrag);
                if destmtu == 0 {
                    break 'done;
                }
            }

            // pf(4) blocked the packet. There is no need to send an ICMP packet back since
            // pf(4) takes care of it.
            Err(Errno::EACCES) => break 'done,

            // a router should not generate ICMP_SOURCEQUENCH as required in RFC1812
            // Requirements for IP Version 4 Routers. source quench could be a big problem
            // under DoS attacks, or the underlying interface is rate-limited.
            Err(Errno::ENOBUFS) => break 'done,

            // ENETUNREACH (shouldn't happen, checked above), EHOSTUNREACH, ENETDOWN,
            // EHOSTDOWN, default:
            Err(_) => {
                type_ = ICMP_UNREACH;
                code = ICMP_UNREACH_HOST;
            }
        }

        let Some(mcopy) = m_gethdr(M_DONTWAIT, MT_DATA) else {
            break 'done;
        };
        mcopy.m_len().set(icmp_len as u32);
        mcopy.m_pkthdr().len.set(icmp_len as i32);
        mcopy
            .m_flags()
            .set(mcopy.m_flags().get() | (mflags & M_COPYFLAGS));
        mcopy.m_pkthdr().ph_rtableid.set(rtableid);
        mcopy.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
        mcopy.m_pkthdr().ph_loopcnt.set(loopcnt);
        let pf = &mcopy.m_pkthdr().pf.flags;
        pf.set(pf.get() | (pfflags & PF_TAG_GENERATED));
        // SAFETY: a fresh packet header mbuf has MHLEN (more than 68) bytes at its data.
        unsafe { ptr::copy_nonoverlapping(icmp_buf.as_ptr(), mcopy.m_data().get(), icmp_len) };
        icmp_error(mcopy, type_, code, dest, destmtu);
    }
    // done:
    if ptr::eq(ro, &iproute) {
        rtfree(ro.ro_rt.get());
    }
}

/// `ip_sysctl`: the `net.inet.ip` sysctls.
pub fn ip_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    // Almost all sysctl names at this level are terminal.
    if name.is_empty() {
        return Err(Errno::ENOTDIR);
    }
    if name.len() != 1 && name[0] != IPCTL_IFQUEUE && name[0] != IPCTL_ARPQUEUE {
        return Err(Errno::ENOTDIR);
    }

    match name[0] {
        IPCTL_SOURCEROUTE => sysctl_securelevel_int(oldp, oldlenp, newp, newlen, &IP_DOSOURCEROUTE),
        IPCTL_MTUDISC => {
            let oldval = ip_mtudisc.load(Ordering::Relaxed);
            let newval = AtomicI32::new(oldval);
            let error = sysctl_int_bounded(oldp, oldlenp, newp, newlen, &newval, 0, 1);
            let newval = newval.into_inner();
            if error.is_ok()
                && oldval != newval
                && ip_mtudisc
                    .compare_exchange(oldval, newval, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
                && newval == 0
            {
                net_lock();
                rt_timer_queue_flush(&IP_MTUDISC_TIMEOUT_Q);
                net_unlock();
            }

            error
        }
        IPCTL_MTUDISCTIMEOUT => {
            let oldval = IP_MTUDISC_TIMEOUT.load(Ordering::Relaxed);
            let newval = AtomicI32::new(oldval);
            let error = sysctl_int_bounded(oldp, oldlenp, newp, newlen, &newval, 0, INT_MAX);
            let newval = newval.into_inner();
            if error.is_ok() && oldval != newval {
                rw_enter_write(&SYSCTL_LOCK);
                IP_MTUDISC_TIMEOUT.store(newval, Ordering::Relaxed);
                rt_timer_queue_change(&IP_MTUDISC_TIMEOUT_Q, newval);
                rw_exit_write(&SYSCTL_LOCK);
            }

            error
        }
        // IPSEC: the IPCTL_ENCDEBUG .. IPCTL_IPSEC_IPCOMP_ALGORITHM names through
        // ipsec_sysctl; not configured, they fall to the table below as unknown names.
        IPCTL_IFQUEUE => sysctl_niq(&name[1..], oldp, oldlenp, newp, newlen, &IPINTRQ),
        IPCTL_ARPQUEUE => sysctl_niq(&name[1..], oldp, oldlenp, newp, newlen, &ARPINQ),
        IPCTL_ARPQUEUED => sysctl_rdint(
            oldp,
            oldlenp,
            newp,
            la_hold_total.load(Ordering::Relaxed) as i32,
        ),
        IPCTL_STATS => ip_sysctl_ipstat(oldp, oldlenp, newp),
        // !MROUTING:
        IPCTL_MRTPROTO | IPCTL_MRTSTATS | IPCTL_MRTMFC | IPCTL_MRTVIF => Err(Errno::EOPNOTSUPP),
        IPCTL_MULTIPATH => {
            let oldval = IPMULTIPATH.load(Ordering::Relaxed);
            let newval = AtomicI32::new(oldval);
            let error = sysctl_int_bounded(oldp, oldlenp, newp, newlen, &newval, 0, 1);
            let newval = newval.into_inner();
            if error.is_ok() && oldval != newval {
                IPMULTIPATH.store(newval, Ordering::Relaxed);
                core::sync::atomic::fence(Ordering::Release);
                crate::net::route::RTGENERATION.fetch_add(1, Ordering::Relaxed);
            }

            error
        }
        IPCTL_IPPORT_FIRSTAUTO
        | IPCTL_IPPORT_LASTAUTO
        | IPCTL_IPPORT_HIFIRSTAUTO
        | IPCTL_IPPORT_HILASTAUTO => Err(unported!("ipport_* (netinet/in_pcb.c)")),
        _ => sysctl_bounded_arr(&IPCTL_VARS, name, oldp, oldlenp, newp, newlen),
    }
}

/// `ip_sysctl_ipstat`: `net.inet.ip.stats`, the counters as a `struct ipstat`.
fn ip_sysctl_ipstat(oldp: usize, oldlenp: &mut usize, newp: usize) -> Result<(), Errno> {
    const N: usize = IpstatCounters::IpsNcounters as usize;
    const _: () = assert!(size_of::<Ipstat>() == N * size_of::<u64>());
    let mut bytes = [0u8; N * size_of::<u64>()];
    for (i, c) in IPCOUNTERS.iter().enumerate() {
        bytes[i * 8..i * 8 + 8].copy_from_slice(&c.load(Ordering::Relaxed).to_ne_bytes());
    }

    sysctl_rdstruct(oldp, oldlenp, newp, &bytes)
}

// ip_savecontrol: struct inpcb and sbcreatecontrol come with the socket layer (see the
// module's deviations).

/// `ip_send_do_dispatch`: sends the packets queued on `xmq` with `ip_output`.
fn ip_send_do_dispatch(xmq: *mut c_void, flags: i32) {
    // SAFETY: the tasks are initialised with their static queue as the argument.
    let mq = unsafe { &*xmq.cast::<MbufQueue>() };
    let ml = MbufList::new();

    mq_delist(mq, &ml);
    if ml_empty(&ml) {
        return;
    }

    net_lock_shared();
    while let Some(m) = ml_dequeue(&ml) {
        let mut ipsecflowinfo = 0u32;

        if let Some(mtag) = m_tag_find(m, crate::sys::mbuf::PACKET_TAG_IPSEC_FLOWINFO, None) {
            // SAFETY: an IPsec flowinfo tag carries a `uint32_t`.
            ipsecflowinfo = unsafe { ptr::read_unaligned(mtag.data().cast::<u32>()) };
            // SAFETY: the tag is on this packet's list.
            unsafe { m_tag_delete(m, mtag) };
        }
        let _ = ip_output(m, None, None, flags, None, ipsecflowinfo);
    }
    net_unlock_shared();
}

/// `ip_sendraw_dispatch`: `ipsendraw_task`.
fn ip_sendraw_dispatch(xmq: *mut c_void) {
    ip_send_do_dispatch(xmq, IP_RAWOUTPUT);
}

/// `ip_send_dispatch`: `ipsend_task`.
fn ip_send_dispatch(xmq: *mut c_void) {
    ip_send_do_dispatch(xmq, 0);
}

/// `ip_send`: queues `m` for `ip_output` from the softnet task.
pub fn ip_send(m: &'static Mbuf) {
    mq_enqueue(&IPSEND_MQ, m);
    if let Some(tq) = net_tq(0) {
        task_add(tq, &IPSEND_TASK);
    }
}

/// `ip_send_raw`: queues `m` (a complete header) for `ip_output` from the softnet task.
pub fn ip_send_raw(m: &'static Mbuf) {
    mq_enqueue(&IPSENDRAW_MQ, m);
    if let Some(tq) = net_tq(0) {
        task_add(tq, &IPSENDRAW_TASK);
    }
}

#[cfg(test)]
pub(crate) mod tests;
