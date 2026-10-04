/*	$OpenBSD: if.h,v 1.224 2026/06/23 14:40:40 bluhm Exp $	*/
/*	$NetBSD: if.h,v 1.23 1996/05/07 02:40:27 thorpej Exp $	*/
/*	$OpenBSD: if.c,v 1.766 2026/09/20 20:50:29 gnezdo Exp $	*/
/*	$NetBSD: if.c,v 1.35 1996/05/07 05:26:04 thorpej Exp $	*/
/* <LICENSES> */
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
 *
 *	@(#)if.h	8.1 (Berkeley) 6/10/93
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
 * Copyright (c) 1980, 1986, 1993
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
 *	@(#)if.c	8.3 (Berkeley) 1/4/94
 */
/* </LICENSES> */

//! Network interfaces: `<net/if.h>` (what user space and the routing socket see) and
//! `net/if.c` (attaching interfaces, the index map, input and output dispatch, `ioctl`s,
//! groups, cloners and the softnet task queues).
//!
//! Upstream: sys/net/if.h @ 3ce1f3f79392
//! Upstream: sys/net/if.c @ 3ce1f3f79392
//!
//! `if` is a Rust keyword, so the module is `if_` (`docs/C_TO_RUST.md`). The kernel's own
//! `struct ifnet` is in `<net/if_var.h>`, not here; this header has the interface flags, the
//! statistics block, the routing-socket messages and the `ioctl(2)` request structures. The
//! structures cross the user/kernel boundary, so they are `#[repr(C)]` with their LP64 sizes
//! checked at compile time. The C's unnamed unions are `#[repr(C)]` unions named after their
//! member (`ifr_ifru` → [`IfrIfru`]), and the C's `#define ifr_flags ifr_ifru.ifru_flags`
//! shorthands are accessor methods (`ifr.ifr_flags()`, `ifr.set_ifr_flags(f)`).
//!
//! `if.c` keeps the interfaces on `ifnetlist` and in an index map (`if_get(index)` takes a
//! reference, `if_put` drops it); attaches them (`if_attach`: an index, a send queue
//! `if_snd` and an input queue `if_rcv` bound to a softnet task queue, the `all` group, the
//! watchdog timeout); passes received packets from the input queues to `if_input` and the
//! protocols in the softnet threads (`if_input_process`, `if_input_proto`); queues packets
//! for transmission (`if_enqueue`); runs the hooks, the interface `ioctl`s and the groups and
//! cloners; and owns `netlock` and the `netisr` word (`if_netisr`).
//!
//! Status: `if.h` `wip` (see the deviations), `if.c` `ported` (M7b).
//!
//! ## Deviations
//! - `CTL_IFQ_NAMES` (a `struct ctlname` table) comes with `<sys/sysctl.h>`.
//! - `LINK_STATE_DESCRIPTIONS` is a slice of [`IfStatusDescription`] whose strings are byte
//!   slices; the C's terminating `{ 0, 0, NULL }` entry is the slice's end.
//! - `IFG_ALL` and `IFG_EGRESS` are byte strings without the NUL.
//! - `#include <net/if_arp.h>` is `crate::net::if_arp`, a module of its own.
//! - `if.c` shares this module with `if.h` (both are `if_` because of the keyword); the
//!   types `if.c` defines (`struct if_idxmap`, `struct softnet`) are private here.
//! - Options and pseudo-devices that are not ported are not configured, and their code is a
//!   comment at each site: `MPLS`, `MROUTING`, `NFSCLIENT`, and `NBRIDGE`, `NCARP`,
//!   `NPPP`, `NPPPOE` as 0. `INET6` (feature `inet6`: `nd6_ifattach`/`nd6_ifdetach`,
//!   `in6_ifattach`/`in6_ifdetach`, `ip6intr`, `ipv6_input`, `ns_tcp6_ml`, the `AF_INET6`
//!   cases, `::1` on the default lo(4) in `if_up`), `NETHER`, `NPF` (pf(4): the interface
//!   and group hooks of `pf_if.c`, `pf_delay_pkt`, `pf_pkt_addr_changed`) and `NBPFILTER`
//!   (bpf(4): the taps of `if_input_local`, `if_vinput` and `p2p_bpf_mtap`, `bpfdetach`)
//!   are configured.
//! - `if_up` takes a `&'static Ifnet` (the C's `struct ifnet *`): `in6_ifattach` links
//!   addresses that keep the interface.
//! - Calls into files that are not ported report themselves with `unported!` and go on as
//!   the C would with an empty subsystem: `inet_ntop` for `AF_INET` (`ifa_print_all`;
//!   `AF_INET6` prints with nd6's `In6Ntop`).
//! - `ifioctl`'s `pru_control` goes through the socket's protocol (`sys/protosw.rs`), as in
//!   C; the kernel's own requests come with a NULL socket and go to `in_ioctl` as privileged
//!   ones (the boot self-test configures an interface that way).
//! - `ifioctl`'s `struct socket *` is a raw pointer (NULL for the kernel's own requests) and
//!   `caddr_t data` a raw pointer: `ifioctl` and the `ioctl` helpers are `unsafe fn`s whose contract is
//!   `sys_ioctl`'s kernel copy of the argument. `SIOCSIFXFLAGS`'s `goto forceup` into the
//!   `SIOCSIFFLAGS` case is a flag checked after the `match`.
//! - SMR (`kern/kern_smr.c`) is not ported: the index map is read without a lock and
//!   replaced under its rwlock, as in C, but `smr_call` (freeing the old map when it grows)
//!   runs at once and `smr_barrier` is empty. That is sound on one CPU with a kernel that is
//!   not preempted: a reader never sleeps between loading the map and using it, so no reader
//!   can be inside a map when its writer frees it. `struct if_idxmap_dtor`, which recycles
//!   the old bitmap as the SMR entry, is not needed; the old bitmap is freed directly.
//! - `if_attach` and `if_attachhead` panic on an interface that already has an index (the C
//!   would corrupt `ifnetlist`); `if_start`'s `KASSERT(if_qstart == if_qstart_compat)` checks
//!   `IFXF_MPSAFE` instead (function pointers do not compare reliably in Rust).
//! - `counters_inc`/`counters_pkt` (`<sys/percpu.h>`) are here, over `if_var.rs`'s
//!   [`IfCounterArray`]; `if_counters_alloc` allocates it with `malloc(M_COUNTERS)`.
//! - The `KERNEL_LOCK()`/`KERNEL_UNLOCK()` pairs and `KERNEL_ASSERT_LOCKED()` are nothing
//!   without `MULTIPROCESSOR`; `NET_TASKQ` is 1 and `softnet_percpu` has nothing to do.
//! - `malloc(M_WAITOK)` that fails panics where the C would sleep (the index map,
//!   `if_alloc_sadl`, `if_attach_queues`, `if_counters_alloc`; `malloc(9)` does not sleep yet).
//! - The hook lists (`if_*hook_add`) and `if_clone_attach`, `ifa_add` are `unsafe fn`s: they
//!   link a caller's object that must stay valid while linked. Out parameters are return
//!   values (`if_clone_lookup` returns the unit with the cloner); `int` results that are
//!   errors are `Result<(), Errno>`, booleans `bool` (`if_isconnected`, `if_congested`,
//!   `niq_enqueue`'s "dropped").

use core::cell::{Cell, UnsafeCell};
use core::cmp::min;
use core::ffi::c_void;
use core::mem::{MaybeUninit, offset_of, size_of};
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicU32, Ordering};

use libkern::{strlcpy, strnlen};

use crate::dev::rnd::enqueue_randomness;
use crate::kern::init_main::{NCPUS, PDEVINIT_DONE};
use crate::kern::kern_clock::TICKS;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_prot::suser;
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write, rw_init};
use crate::kern::kern_synch::{
    refcnt_finalize, refcnt_init, refcnt_rele, refcnt_rele_wake, refcnt_take,
};
use crate::kern::kern_task::{task_add, task_del, task_set, taskq_create};
use crate::kern::kern_tc::getmicrotime;
use crate::kern::kern_timeout::{timeout_add_sec, timeout_del, timeout_set};
use crate::kern::sched_bsd::r#yield;
use crate::kern::subr_prf::{Str, panic, panicstr, printf, snprintf};
use crate::kern::uipc_mbuf::{
    m_freem, m_resethdr, ml_dequeue, ml_enqueue, ml_init, ml_purge, mq_delist, mq_enqueue, mq_purge,
};
use crate::machine::Machine;
use crate::machine::copy::{copyin, copyinstr, copyout, copyoutstr};
use crate::machine::cpu::{Cpu, curcpu};
use crate::machine::intr::{IPL_NET, IPL_NONE, splnet, splx};
use crate::net::bpf::{
    BPF_DIRECTION_IN, BPF_DIRECTION_OUT, bpf_mtap_af, bpf_mtap_ether, bpfdetach,
};
use crate::net::if_dl::{SockaddrDl, lladdr};
use crate::net::if_ethersubr::ether_brport_isset;
use crate::net::if_types::{IFT_CARP, IFT_ETHER, IFT_IEEE80211, IFT_ISO88025, IFT_PPP, IFT_XETHER};
use crate::net::if_var::{
    IF_TXMIT_DEFAULT, IFC_NCOUNTERS, IFNET_SLOWTIMO, IfClone, IfClones, IfCounterArray, IfCounters,
    IfInputFn, IfTmplist, IfaTmplist, Ifaddr, IfgGroup, IfgHead, IfgList, IfgMember, IfgTmplist,
    Ifnet, IfnetHead, Netstack, Niqueue, if_input_process_proto,
};
use crate::net::ifq::{
    Ifiqueue, IfqOps, Ifqueue, ifiq_add_data, ifiq_destroy, ifiq_enqueue, ifiq_enqueue_qlim,
    ifiq_init, ifiq_input, ifq_add_data, ifq_attach, ifq_barrier, ifq_clr_oactive, ifq_destroy,
    ifq_enqueue, ifq_idx, ifq_init, ifq_init_maxlen, ifq_is_oactive, ifq_purge, ifq_start,
};
#[cfg(feature = "inet6")]
use crate::net::netisr::NETISR_IPV6;
use crate::net::netisr::{NETISR_ARP, NETISR_IP, schednetisr};
use crate::net::pf::{pf_delay_pkt, pf_pkt_addr_changed};
use crate::net::pf_if::{
    pfi_attach_ifgroup, pfi_attach_ifnet, pfi_detach_ifgroup, pfi_detach_ifnet,
    pfi_group_addmember, pfi_group_delmember,
};
use crate::net::route::{
    RTF_BLACKHOLE, RTF_LLINFO, RTF_LOCAL, RTF_REJECT, RTLABEL_LEN, RTM_ADD, RTP_ANY, Rtentry,
    ifafree, ifaref, rt_if_track, rtlabel_id2name, rtlabel_name2id, rtlabel_unref,
};
use crate::net::rtable::{
    rt_key, rtable_add, rtable_empty, rtable_exists, rtable_iterate, rtable_l2, rtable_l2set,
    rtable_loindex, rtable_lookup,
};
use crate::net::rtsock::{rtm_ifannounce, rtm_ifchg};
use crate::netinet::if_ether::{ETHER_ADDR_LEN, arpcom_of, arpintr, ether_is_multicast};
use crate::netinet::igmp::rti_delete;
use crate::netinet::in_::{INADDR_ANY, SockaddrIn, in_ifdetach, in_ioctl, satosin_const, sintosa};
use crate::netinet::ip_input::{ipintr, ipv4_input};
use crate::netinet::ip_output::{in_hdr_cksum_out, in_proto_cksum_out};
use crate::netinet::tcp_input::tcp_input_mlist;
use crate::netinet::tcp_output::tcp_if_output_tso;
use crate::netinet::tcp_var::{TcpstatCounters, tcpstat_inc};
#[cfg(feature = "inet6")]
use crate::netinet6::in6::{
    IN6ADDR_ANY, SA6_ANY, in6_are_addr_equal, in6_purgeaddr, in6ifa_ifpforlinklocal,
    satosin6_const, sin6tosa,
};
#[cfg(feature = "inet6")]
use crate::netinet6::in6_ifattach::{in6_ifattach, in6_ifdetach};
#[cfg(feature = "inet6")]
use crate::netinet6::in6_proto::IP6_FORWARDING;
#[cfg(feature = "inet6")]
use crate::netinet6::in6_var::{SIOCAIFADDR_IN6, SIOCDIFADDR_IN6};
#[cfg(feature = "inet6")]
use crate::netinet6::ip6_input::{ip6intr, ipv6_input};
#[cfg(feature = "inet6")]
use crate::netinet6::ip6_output::in6_proto_cksum_out;
#[cfg(feature = "inet6")]
use crate::netinet6::nd6::{In6Ntop, nd6_ifattach, nd6_ifdetach};
use crate::sys::errno::Errno;
use crate::sys::ioccom::iocparm_len;
use crate::sys::kernel::HZ;
use crate::sys::limits::USHRT_MAX;
use crate::sys::malloc::{M_COUNTERS, M_DEVBUF, M_IFADDR, M_IFGROUP, M_NOWAIT, M_WAITOK, M_ZERO};
use crate::sys::mbuf::{
    M_BCAST, M_FLOWID, M_ICMP_CSUM_IN_OK, M_ICMP_CSUM_OUT, M_IPV4_CSUM_IN_OK, M_IPV4_CSUM_OUT,
    M_LOOP, M_MCAST, M_TCP_CSUM_IN_OK, M_TCP_CSUM_OUT, M_TCP_TSO, M_TIMESTAMP, M_UDP_CSUM_IN_OK,
    M_UDP_CSUM_OUT, Mbuf, MbufList, MbufQueue, mbuf_list_first, ml_empty, ml_len, mq_len,
};
use crate::sys::mutex::Mutex;
use crate::sys::param::{clrbit, howmany, isclr, isset, setbit};
use crate::sys::proc::Proc;
use crate::sys::protosw;
use crate::sys::queue::{ListHead, TailqEntry, TailqHead};
use crate::sys::refcnt::Refcnt;
use crate::sys::rwlock::{DT_RWLOCK_IDX_NETLOCK, Rwlock};
use crate::sys::sched::SPCF_SHOULDYIELD;
use crate::sys::socket::{
    AF_INET, AF_INET6, AF_LINK, AF_MAX, RT_TABLEID_MAX, Sockaddr, SockaddrStorage,
};
use crate::sys::socketvar::Socket;
use crate::sys::sockio::*;
use crate::sys::systm::{
    net_assert_locked, net_assert_locked_exclusive, net_lock, net_lock_shared, net_unlock,
    net_unlock_shared,
};
use crate::sys::task::{TASKQ_MPSAFE, Task, TaskList, Taskq};
use crate::sys::time::Timeval;
use crate::sys::types::SaFamily;
use crate::{kassert, unported};

/// Length of interface external name, including terminating '\0'. Note: this is the same size
/// as a generic device's external name.
pub const IF_NAMESIZE: usize = 16;

/// Number of cluster pools.
pub const MCLPOOLS: usize = 8;

/// `IFQ_NQUEUES`: priority queues per interface queue.
pub const IFQ_NQUEUES: u32 = 8;
/// `IFQ_MINPRIO`.
pub const IFQ_MINPRIO: u32 = 0;
/// `IFQ_MAXPRIO`.
pub const IFQ_MAXPRIO: u32 = IFQ_NQUEUES - 1;
/// `IFQ_DEFPRIO`.
pub const IFQ_DEFPRIO: u32 = 3;

// Values for if_link_state.

/// Link unknown.
pub const LINK_STATE_UNKNOWN: u8 = 0;
/// Link invalid.
pub const LINK_STATE_INVALID: u8 = 1;
/// Link is down.
pub const LINK_STATE_DOWN: u8 = 2;
/// Keepalive reports down.
pub const LINK_STATE_KALIVE_DOWN: u8 = 3;
/// Link is up.
pub const LINK_STATE_UP: u8 = 4;
/// Link is up and half duplex.
pub const LINK_STATE_HALF_DUPLEX: u8 = 5;
/// Link is up and full duplex.
pub const LINK_STATE_FULL_DUPLEX: u8 = 6;

/// Traditional BSD name for length of interface external name.
pub const IFNAMSIZ: usize = IF_NAMESIZE;

/// Length of interface description, including terminating '\0'.
pub const IFDESCRSIZE: usize = 64;

// Interface flags can be either owned by the stack or the driver. The comments say who toggles
// which flag: [I] immutable after creation, [N] written by the stack (upon user request), [d]
// written by the driver, [c] for userland compatibility only.

/// [N] interface is up.
pub const IFF_UP: i32 = 0x1;
/// [I] broadcast address valid.
pub const IFF_BROADCAST: i32 = 0x2;
/// [N] turn on debugging.
pub const IFF_DEBUG: i32 = 0x4;
/// [I] is a loopback net.
pub const IFF_LOOPBACK: i32 = 0x8;
/// [I] is point-to-point link.
pub const IFF_POINTOPOINT: i32 = 0x10;
/// [N] only static ARP.
pub const IFF_STATICARP: i32 = 0x20;
/// [d] resources allocated.
pub const IFF_RUNNING: i32 = 0x40;
/// [N] no address resolution protocol.
pub const IFF_NOARP: i32 = 0x80;
/// [N] receive all packets.
pub const IFF_PROMISC: i32 = 0x100;
/// [d] receive all multicast packets.
pub const IFF_ALLMULTI: i32 = 0x200;
/// [c] transmission in progress.
pub const IFF_OACTIVE: i32 = 0x400;
/// [I] can't hear own transmissions.
pub const IFF_SIMPLEX: i32 = 0x800;
/// [N] per link layer defined bit.
pub const IFF_LINK0: i32 = 0x1000;
/// [N] per link layer defined bit.
pub const IFF_LINK1: i32 = 0x2000;
/// [N] per link layer defined bit.
pub const IFF_LINK2: i32 = 0x4000;
/// [I] supports multicast.
pub const IFF_MULTICAST: i32 = 0x8000;

/// Flags set internally only.
pub const IFF_CANTCHANGE: i32 = IFF_BROADCAST
    | IFF_LOOPBACK
    | IFF_POINTOPOINT
    | IFF_RUNNING
    | IFF_OACTIVE
    | IFF_SIMPLEX
    | IFF_MULTICAST
    | IFF_ALLMULTI;

/// [I] `if_start` is mpsafe.
pub const IFXF_MPSAFE: i32 = 0x1;
/// [I] pseudo interface.
pub const IFXF_CLONED: i32 = 0x2;
/// [N] v6 temporary addrs enabled.
pub const IFXF_AUTOCONF6TEMP: i32 = 0x4;
/// [N] supports MPLS.
pub const IFXF_MPLS: i32 = 0x8;
/// [N] wake on lan enabled.
pub const IFXF_WOL: i32 = 0x10;
/// [N] v6 autoconf enabled.
pub const IFXF_AUTOCONF6: i32 = 0x20;
/// [N] don't do RFC 7217.
pub const IFXF_INET6_NOSOII: i32 = 0x40;
/// [N] v4 autoconf (aka dhcp) enabled.
pub const IFXF_AUTOCONF4: i32 = 0x80;
/// [N] only used for bpf.
pub const IFXF_MONITOR: i32 = 0x100;
/// [N] TCP large recv offload.
pub const IFXF_LRO: i32 = 0x200;
/// [I] mbuf with 64 bit DMA supported.
pub const IFXF_MBUF_64BIT: i32 = 0x400;

/// Extended flags the user cannot change.
pub const IFXF_CANTCHANGE: i32 = IFXF_MPSAFE | IFXF_CLONED;

// Capabilities that interfaces can advertise.

/// Can do IPv4 header csum.
#[allow(non_upper_case_globals)] // the C name
pub const IFCAP_CSUM_IPv4: u32 = 0x0000_0001;
/// Can do IPv4/TCP csum.
#[allow(non_upper_case_globals)] // the C name
pub const IFCAP_CSUM_TCPv4: u32 = 0x0000_0002;
/// Can do IPv4/UDP csum.
#[allow(non_upper_case_globals)] // the C name
pub const IFCAP_CSUM_UDPv4: u32 = 0x0000_0004;
/// VLAN-compatible MTU.
pub const IFCAP_VLAN_MTU: u32 = 0x0000_0010;
/// Hardware VLAN tag support.
pub const IFCAP_VLAN_HWTAGGING: u32 = 0x0000_0020;
/// HW offload w/ inline tag.
pub const IFCAP_VLAN_HWOFFLOAD: u32 = 0x0000_0040;
/// Can do IPv6/TCP checksums.
#[allow(non_upper_case_globals)] // the C name
pub const IFCAP_CSUM_TCPv6: u32 = 0x0000_0080;
/// Can do IPv6/UDP checksums.
#[allow(non_upper_case_globals)] // the C name
pub const IFCAP_CSUM_UDPv6: u32 = 0x0000_0100;
/// IPv4/TCP segment offload.
#[allow(non_upper_case_globals)] // the C name
pub const IFCAP_TSOv4: u32 = 0x0000_1000;
/// IPv6/TCP segment offload.
#[allow(non_upper_case_globals)] // the C name
pub const IFCAP_TSOv6: u32 = 0x0000_2000;
/// TCP large recv offload.
pub const IFCAP_LRO: u32 = 0x0000_4000;
/// Can do wake on lan.
pub const IFCAP_WOL: u32 = 0x0000_8000;

/// The checksum offload capabilities.
pub const IFCAP_CSUM_MASK: u32 =
    IFCAP_CSUM_IPv4 | IFCAP_CSUM_TCPv4 | IFCAP_CSUM_UDPv4 | IFCAP_CSUM_TCPv6 | IFCAP_CSUM_UDPv6;

// Symbolic names for terminal (per-protocol) CTL_IFQ_ nodes.

/// `IFQCTL_LEN`.
pub const IFQCTL_LEN: i32 = 1;
/// `IFQCTL_MAXLEN`.
pub const IFQCTL_MAXLEN: i32 = 2;
/// `IFQCTL_DROPS`.
pub const IFQCTL_DROPS: i32 = 3;
/// `IFQCTL_CONGESTION`.
pub const IFQCTL_CONGESTION: i32 = 4;
/// `IFQCTL_MAXID`.
pub const IFQCTL_MAXID: i32 = 5;

/// Interface arrival.
pub const IFAN_ARRIVAL: u16 = 0;
/// Interface departure.
pub const IFAN_DEPARTURE: u16 = 1;

/// Group contains all interfaces.
pub const IFG_ALL: &[u8] = b"all";
/// If(s) default route(s) point to.
pub const IFG_EGRESS: &[u8] = b"egress";

/// `IF_HDRPRIO_MIN`.
pub const IF_HDRPRIO_MIN: i32 = IFQ_MINPRIO as i32;
/// `IF_HDRPRIO_MAX`.
pub const IF_HDRPRIO_MAX: i32 = IFQ_MAXPRIO as i32;
/// Use mbuf prio.
pub const IF_HDRPRIO_PACKET: i32 = -1;
/// Copy payload prio.
pub const IF_HDRPRIO_PAYLOAD: i32 = -2;
/// Use outer prio.
pub const IF_HDRPRIO_OUTER: i32 = -3;

/// Ethernet or ethernet tagged.
pub const IF_PWE3_ETHERNET: i32 = 1;
/// IP layer 2.
pub const IF_PWE3_IP: i32 = 2;

/// In: prefix given; out: kernel fills id.
pub const IFLR_PREFIX: u32 = 0x8000;

/// `IFSFF_ADDR_EEPROM`.
pub const IFSFF_ADDR_EEPROM: u8 = 0xa0;
/// `IFSFF_ADDR_DDM`.
pub const IFSFF_ADDR_DDM: u8 = 0xa2;

/// `IFSFF_DATA_LEN`.
pub const IFSFF_DATA_LEN: usize = 256;

/// `IF_MAX_VECTORS`: most interrupt vectors (and queues) one interface uses.
pub const IF_MAX_VECTORS: usize = 8;

/// `struct if_nameindex`: one entry of `if_nameindex(3)`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct IfNameindex {
    /// Interface index.
    pub if_index: u32,
    /// Interface name.
    pub if_name: *mut u8,
}

/// `struct if_clonereq`: structure used to query names of interface cloners.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct IfClonereq {
    /// Total cloners (out).
    pub ifcr_total: i32,
    /// Room for this many in user buffer.
    pub ifcr_count: i32,
    /// Buffer for cloner names.
    pub ifcr_buffer: *mut u8,
}

/// `struct if_rxring`: a receive ring's watermarks.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfRxring {
    /// `rxr_adjusted`.
    pub rxr_adjusted: i32,
    /// `rxr_alive`.
    pub rxr_alive: u32,
    /// Current watermark.
    pub rxr_cwm: u32,
    /// Low watermark.
    pub rxr_lwm: u32,
    /// High watermark.
    pub rxr_hwm: u32,
}

/// `struct if_rxring_info`: one ring of a `SIOCGIFRXR` answer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfRxringInfo {
    /// Name of the ring.
    pub ifr_name: [u8; 16],
    /// Size of the packets on the ring.
    pub ifr_size: u32,
    /// The ring's watermarks.
    pub ifr_info: IfRxring,
}

/// `struct if_rxrinfo`: structure used in `SIOCGIFRXR` request.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct IfRxrinfo {
    /// `ifri_total`.
    pub ifri_total: u32,
    /// `ifri_entries`.
    pub ifri_entries: *mut IfRxringInfo,
}

/// `struct if_data`: structure defining statistics and other data kept regarding a network
/// interface.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfData {
    // generic interface information
    /// Ethernet, tokenring, etc.
    pub ifi_type: u8,
    /// Media address length.
    pub ifi_addrlen: u8,
    /// Media header length.
    pub ifi_hdrlen: u8,
    /// Current link state.
    pub ifi_link_state: u8,
    /// Maximum transmission unit.
    pub ifi_mtu: u32,
    /// Routing metric (external only).
    pub ifi_metric: u32,
    /// Routing instance.
    pub ifi_rdomain: u32,
    /// Linespeed.
    pub ifi_baudrate: u64,
    // volatile statistics
    /// Packets received on interface.
    pub ifi_ipackets: u64,
    /// Input errors on interface.
    pub ifi_ierrors: u64,
    /// Packets sent on interface.
    pub ifi_opackets: u64,
    /// Output errors on interface.
    pub ifi_oerrors: u64,
    /// Collisions on csma interfaces.
    pub ifi_collisions: u64,
    /// Total number of octets received.
    pub ifi_ibytes: u64,
    /// Total number of octets sent.
    pub ifi_obytes: u64,
    /// Packets received via multicast.
    pub ifi_imcasts: u64,
    /// Packets sent via multicast.
    pub ifi_omcasts: u64,
    /// Dropped on input, this interface.
    pub ifi_iqdrops: u64,
    /// Dropped on output, this interface.
    pub ifi_oqdrops: u64,
    /// Destined for unsupported protocol.
    pub ifi_noproto: u64,
    /// Interface capabilities.
    pub ifi_capabilities: u32,
    /// Last operational state change.
    pub ifi_lastchange: Timeval,
}

/// `struct if_status_description`: status bit descriptions for the various interface types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IfStatusDescription {
    /// Interface type the description applies to, 0 for any.
    pub ifs_type: u8,
    /// Link state.
    pub ifs_state: u8,
    /// The description.
    pub ifs_string: &'static [u8],
}

/// `struct if_msghdr`: message format for use in obtaining information about interfaces from
/// sysctl and the routing socket.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfMsghdr {
    /// To skip over non-understood messages.
    pub ifm_msglen: u16,
    /// Future binary compatibility.
    pub ifm_version: u8,
    /// Message type.
    pub ifm_type: u8,
    /// sizeof(if_msghdr) to skip over the header.
    pub ifm_hdrlen: u16,
    /// Index for associated ifp.
    pub ifm_index: u16,
    /// Routing table id.
    pub ifm_tableid: u16,
    /// Padding.
    pub ifm_pad1: u8,
    /// Padding.
    pub ifm_pad2: u8,
    /// Like `rtm_addrs`.
    pub ifm_addrs: i32,
    /// Value of `if_flags`.
    pub ifm_flags: i32,
    /// Value of `if_xflags`.
    pub ifm_xflags: i32,
    /// Statistics and other data about if.
    pub ifm_data: IfData,
}

/// `struct ifa_msghdr`: message format for use in obtaining information about interface
/// addresses from sysctl and the routing socket.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfaMsghdr {
    /// To skip over non-understood messages.
    pub ifam_msglen: u16,
    /// Future binary compatibility.
    pub ifam_version: u8,
    /// Message type.
    pub ifam_type: u8,
    /// sizeof(ifa_msghdr) to skip over the header.
    pub ifam_hdrlen: u16,
    /// Index for associated ifp.
    pub ifam_index: u16,
    /// Routing table id.
    pub ifam_tableid: u16,
    /// Padding.
    pub ifam_pad1: u8,
    /// Padding.
    pub ifam_pad2: u8,
    /// Like `rtm_addrs`.
    pub ifam_addrs: i32,
    /// Value of `ifa_flags`.
    pub ifam_flags: i32,
    /// Value of `ifa_metric`.
    pub ifam_metric: i32,
}

/// `struct if_announcemsghdr`: message format announcing the arrival or departure of a network
/// interface.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfAnnouncemsghdr {
    /// To skip over non-understood messages.
    pub ifan_msglen: u16,
    /// Future binary compatibility.
    pub ifan_version: u8,
    /// Message type.
    pub ifan_type: u8,
    /// sizeof(if_announcemsghdr) to skip header.
    pub ifan_hdrlen: u16,
    /// Index for associated ifp.
    pub ifan_index: u16,
    /// What type of announcement.
    pub ifan_what: u16,
    /// If name, e.g. "en0".
    pub ifan_name: [u8; IFNAMSIZ],
}

/// `struct if_ieee80211_data`: message format used to pass 80211 interface info.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfIeee80211Data {
    /// `IEEE80211_CHAN_MAX` == 255.
    pub ifie_channel: u8,
    /// Length of `ifie_nwid`.
    pub ifie_nwid_len: u8,
    /// `ieee80211com.ic_flags`.
    pub ifie_flags: u32,
    /// `ieee80211com.ic_xflags`.
    pub ifie_xflags: u32,
    /// `IEEE80211_NWID_LEN`.
    pub ifie_nwid: [u8; 32],
    /// `IEEE80211_ADDR_LEN`.
    pub ifie_addr: [u8; 6],
}

/// `struct if_ieee80211_msghdr`: the routing message carrying [`IfIeee80211Data`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfIeee80211Msghdr {
    /// To skip over non-understood messages.
    pub ifim_msglen: u16,
    /// Future binary compatibility.
    pub ifim_version: u8,
    /// Message type.
    pub ifim_type: u8,
    /// sizeof(if_ieee80211_msghdr) to skip over the header.
    pub ifim_hdrlen: u16,
    /// Index for associated ifp.
    pub ifim_index: u16,
    /// Routing table id.
    pub ifim_tableid: u16,
    /// The interface data.
    pub ifim_ifie: IfIeee80211Data,
}

/// `struct if_nameindex_msg`: message format used to pass interface name to index mappings.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfNameindexMsg {
    /// Interface index.
    pub if_index: u32,
    /// Interface name.
    pub if_name: [u8; IFNAMSIZ],
}

/// The unnamed union of `struct ifg_req`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union IfgrqIfgrqu {
    /// A group name.
    pub ifgrqu_group: [u8; IFNAMSIZ],
    /// A member name.
    pub ifgrqu_member: [u8; IFNAMSIZ],
}

/// `struct ifg_req`: one group (or member) name of a `SIOCGIFGROUP`/`SIOCGIFGMEMB` answer.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IfgReq {
    /// The name.
    pub ifgrq_ifgrqu: IfgrqIfgrqu,
}

impl IfgReq {
    /// `ifgrq_group`.
    pub fn ifgrq_group(&self) -> &[u8; IFNAMSIZ] {
        // SAFETY: both members are the same byte array; every bit pattern is valid.
        unsafe { &self.ifgrq_ifgrqu.ifgrqu_group }
    }

    /// `ifgrq_member`.
    pub fn ifgrq_member(&self) -> &[u8; IFNAMSIZ] {
        // SAFETY: both members are the same byte array; every bit pattern is valid.
        unsafe { &self.ifgrq_ifgrqu.ifgrqu_member }
    }
}

/// `struct ifg_attrib`: attributes of an interface group.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfgAttrib {
    /// CARP demotion counter.
    pub ifg_carp_demoted: i32,
}

/// The unnamed union of `struct ifgroupreq`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union IfgrIfgru {
    /// A group name.
    pub ifgru_group: [u8; IFNAMSIZ],
    /// The user's array of group requests.
    pub ifgru_groups: *mut IfgReq,
    /// The group's attributes.
    pub ifgru_attrib: IfgAttrib,
}

/// `struct ifgroupreq`: used to lookup groups for an interface.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ifgroupreq {
    /// Interface name.
    pub ifgr_name: [u8; IFNAMSIZ],
    /// Length of the `ifgr_groups` buffer.
    pub ifgr_len: u32,
    /// The request's argument.
    pub ifgr_ifgru: IfgrIfgru,
}

impl Ifgroupreq {
    /// `ifgr_group`.
    pub fn ifgr_group(&self) -> &[u8; IFNAMSIZ] {
        // SAFETY: the union's members are plain data; every bit pattern is a valid byte array.
        unsafe { &self.ifgr_ifgru.ifgru_group }
    }

    /// `ifgr_group`, writable.
    pub fn ifgr_group_mut(&mut self) -> &mut [u8; IFNAMSIZ] {
        // SAFETY: as above; writing bytes cannot make another member invalid.
        unsafe { &mut self.ifgr_ifgru.ifgru_group }
    }

    /// `ifgr_groups`: the user address of the group array.
    pub fn ifgr_groups(&self) -> *mut IfgReq {
        // SAFETY: every bit pattern is a valid raw pointer; it is not dereferenced here.
        unsafe { self.ifgr_ifgru.ifgru_groups }
    }

    /// `ifgr_attrib`.
    pub fn ifgr_attrib(&self) -> IfgAttrib {
        // SAFETY: every bit pattern is a valid `int`.
        unsafe { self.ifgr_ifgru.ifgru_attrib }
    }
}

/// The unnamed union of `struct ifreq`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union IfrIfru {
    /// Address.
    pub ifru_addr: Sockaddr,
    /// Other end of p-to-p link.
    pub ifru_dstaddr: Sockaddr,
    /// Broadcast address.
    pub ifru_broadaddr: Sockaddr,
    /// Flags.
    pub ifru_flags: i16,
    /// Metric, and the other `int` overloads.
    pub ifru_metric: i32,
    /// Virtual Net Id.
    pub ifru_vnetid: i64,
    /// Media options.
    pub ifru_media: u64,
    /// For use by interface (a user address).
    pub ifru_data: *mut u8,
    /// Interface index.
    pub ifru_index: u32,
}

/// `struct ifreq`: interface request structure used for socket ioctl's. All interface ioctl's
/// must have parameter definitions which begin with `ifr_name`. The remainder may be interface
/// specific.
///
/// Every member of the union is plain data, valid for any bit pattern, so the accessors are
/// safe; they are the C's `ifr_*` shorthands.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ifreq {
    /// If name, e.g. "en0".
    pub ifr_name: [u8; IFNAMSIZ],
    /// The request's argument.
    pub ifr_ifru: IfrIfru,
}

/// Defines an `ifreq` accessor pair for a `Copy` member of the union.
macro_rules! ifr_accessor {
    ($(#[$doc:meta] $get:ident, $set:ident => $field:ident: $ty:ty;)*) => {
        $(
            #[$doc]
            pub fn $get(&self) -> $ty {
                // SAFETY: every member of `ifr_ifru` is plain data, valid for any bit pattern.
                unsafe { self.ifr_ifru.$field }
            }

            #[$doc]
            pub fn $set(&mut self, v: $ty) {
                self.ifr_ifru.$field = v;
            }
        )*
    };
}

impl Ifreq {
    /// An all-zero request.
    pub const fn zeroed() -> Self {
        Self {
            ifr_name: [0; IFNAMSIZ],
            ifr_ifru: IfrIfru {
                ifru_addr: Sockaddr {
                    sa_len: 0,
                    sa_family: 0,
                    sa_data: [0; 14],
                },
            },
        }
    }

    /// `ifr_addr`: address.
    pub fn ifr_addr(&self) -> &Sockaddr {
        // SAFETY: every member of `ifr_ifru` is plain data, valid for any bit pattern.
        unsafe { &self.ifr_ifru.ifru_addr }
    }

    /// `ifr_addr`, writable.
    pub fn ifr_addr_mut(&mut self) -> &mut Sockaddr {
        // SAFETY: as above; a `Sockaddr` is bytes, so writing it leaves every member valid.
        unsafe { &mut self.ifr_ifru.ifru_addr }
    }

    /// `ifr_dstaddr`: other end of p-to-p link.
    pub fn ifr_dstaddr(&self) -> &Sockaddr {
        // SAFETY: every member of `ifr_ifru` is plain data, valid for any bit pattern.
        unsafe { &self.ifr_ifru.ifru_dstaddr }
    }

    /// `ifr_dstaddr`, writable.
    pub fn ifr_dstaddr_mut(&mut self) -> &mut Sockaddr {
        // SAFETY: as for `ifr_addr_mut`.
        unsafe { &mut self.ifr_ifru.ifru_dstaddr }
    }

    /// `ifr_broadaddr`: broadcast address.
    pub fn ifr_broadaddr(&self) -> &Sockaddr {
        // SAFETY: every member of `ifr_ifru` is plain data, valid for any bit pattern.
        unsafe { &self.ifr_ifru.ifru_broadaddr }
    }

    /// `ifr_broadaddr`, writable.
    pub fn ifr_broadaddr_mut(&mut self) -> &mut Sockaddr {
        // SAFETY: as for `ifr_addr_mut`.
        unsafe { &mut self.ifr_ifru.ifru_broadaddr }
    }

    ifr_accessor! {
        /// `ifr_flags`: flags.
        ifr_flags, set_ifr_flags => ifru_flags: i16;
        /// `ifr_metric`: metric.
        ifr_metric, set_ifr_metric => ifru_metric: i32;
        /// `ifr_mtu`: mtu (overload).
        ifr_mtu, set_ifr_mtu => ifru_metric: i32;
        /// `ifr_hardmtu`: hardmtu (overload).
        ifr_hardmtu, set_ifr_hardmtu => ifru_metric: i32;
        /// `ifr_media`: media options.
        ifr_media, set_ifr_media => ifru_media: u64;
        /// `ifr_rdomainid`: VRF instance (overload).
        ifr_rdomainid, set_ifr_rdomainid => ifru_metric: i32;
        /// `ifr_vnetid`: Virtual Net Id.
        ifr_vnetid, set_ifr_vnetid => ifru_vnetid: i64;
        /// `ifr_ttl`: tunnel TTL (overload).
        ifr_ttl, set_ifr_ttl => ifru_metric: i32;
        /// `ifr_df`: tunnel DF (overload).
        ifr_df, set_ifr_df => ifru_metric: i32;
        /// `ifr_data`: for use by interface.
        ifr_data, set_ifr_data => ifru_data: *mut u8;
        /// `ifr_index`: interface index.
        ifr_index, set_ifr_index => ifru_index: u32;
        /// `ifr_llprio`: link layer priority.
        ifr_llprio, set_ifr_llprio => ifru_metric: i32;
        /// `ifr_hdrprio`: header prio field config.
        ifr_hdrprio, set_ifr_hdrprio => ifru_metric: i32;
        /// `ifr_pwe3`: PWE3 type.
        ifr_pwe3, set_ifr_pwe3 => ifru_metric: i32;
    }
}

/// The unnamed union of `struct ifaliasreq`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union IfraIfrau {
    /// The address.
    pub ifrau_addr: Sockaddr,
    /// Forces `int` alignment.
    pub ifrau_align: i32,
}

/// `struct ifaliasreq`: the argument of `SIOCAIFADDR`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ifaliasreq {
    /// If name, e.g. "en0".
    pub ifra_name: [u8; IFNAMSIZ],
    /// The address (`ifra_addr`).
    pub ifra_ifrau: IfraIfrau,
    /// Destination (or broadcast, `ifra_broadaddr`) address.
    pub ifra_dstaddr: Sockaddr,
    /// Netmask.
    pub ifra_mask: Sockaddr,
}

impl Ifaliasreq {
    /// `ifra_addr`.
    pub fn ifra_addr(&self) -> &Sockaddr {
        // SAFETY: both members are plain data, valid for any bit pattern.
        unsafe { &self.ifra_ifrau.ifrau_addr }
    }

    /// `ifra_addr`, writable.
    pub fn ifra_addr_mut(&mut self) -> &mut Sockaddr {
        // SAFETY: as above.
        unsafe { &mut self.ifra_ifrau.ifrau_addr }
    }

    /// `ifra_broadaddr`: the same field as `ifra_dstaddr`.
    pub fn ifra_broadaddr(&self) -> &Sockaddr {
        &self.ifra_dstaddr
    }
}

/// `struct ifmediareq`: the argument of `SIOCGIFMEDIA`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Ifmediareq {
    /// If name, e.g. "en0".
    pub ifm_name: [u8; IFNAMSIZ],
    /// Get/set current media options.
    pub ifm_current: u64,
    /// Don't care mask.
    pub ifm_mask: u64,
    /// Media status.
    pub ifm_status: u64,
    /// Active options.
    pub ifm_active: u64,
    /// # entries in `ifm_ulist` array.
    pub ifm_count: i32,
    /// Media words.
    pub ifm_ulist: *mut u64,
}

/// `struct ifkalivereq`: the argument of `SIOCSETKALIVE`/`SIOCGETKALIVE`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ifkalivereq {
    /// If name, e.g. "en0".
    pub ikar_name: [u8; IFNAMSIZ],
    /// Keepalive timeout.
    pub ikar_timeo: i32,
    /// Keepalive count.
    pub ikar_cnt: i32,
}

/// The unnamed union of `struct ifconf`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union IfcIfcu {
    /// Buffer address.
    pub ifcu_buf: *mut u8,
    /// Array of structures returned.
    pub ifcu_req: *mut Ifreq,
}

/// `struct ifconf`: structure used in `SIOCGIFCONF` request. Used to retrieve interface
/// configuration for machine (useful for programs which must know all networks accessible).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ifconf {
    /// Size of associated buffer.
    pub ifc_len: i32,
    /// The buffer.
    pub ifc_ifcu: IfcIfcu,
}

impl Ifconf {
    /// `ifc_buf`: buffer address.
    pub fn ifc_buf(&self) -> *mut u8 {
        // SAFETY: both members are raw pointers, valid for any bit pattern.
        unsafe { self.ifc_ifcu.ifcu_buf }
    }

    /// `ifc_req`: array of structures returned.
    pub fn ifc_req(&self) -> *mut Ifreq {
        // SAFETY: both members are raw pointers, valid for any bit pattern.
        unsafe { self.ifc_ifcu.ifcu_req }
    }
}

/// `struct if_laddrreq`: structure for `SIOC[AGD]LIFADDR`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IfLaddrreq {
    /// Interface name.
    pub iflr_name: [u8; IFNAMSIZ],
    /// `IFLR_PREFIX`.
    pub flags: u32,
    /// In/out.
    pub prefixlen: u32,
    /// In/out.
    pub addr: SockaddrStorage,
    /// Out.
    pub dstaddr: SockaddrStorage,
}

/// `struct if_afreq`: `SIOCIFAFDETACH`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfAfreq {
    /// Interface name.
    pub ifar_name: [u8; IFNAMSIZ],
    /// Address family.
    pub ifar_af: SaFamily,
}

/// `struct if_parent`: `SIOC[SG]IFPARENT`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfParent {
    /// Interface name.
    pub ifp_name: [u8; IFNAMSIZ],
    /// Parent interface name.
    pub ifp_parent: [u8; IFNAMSIZ],
}

/// `struct if_sffpage`: `SIOCGIFSFFPAGE`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IfSffpage {
    /// u -> k.
    pub sff_ifname: [u8; IFNAMSIZ],
    /// u -> k.
    pub sff_addr: u8,
    /// u -> k.
    pub sff_page: u8,
    /// k -> u.
    pub sff_data: [u8; IFSFF_DATA_LEN],
}

/// `LINK_STATE_DESCRIPTIONS`: how `ifconfig(8)` names a link state, per interface type (0
/// matches any type).
pub const LINK_STATE_DESCRIPTIONS: &[IfStatusDescription] = &[
    lsd(IFT_ETHER, LINK_STATE_DOWN, b"no carrier"),
    lsd(IFT_IEEE80211, LINK_STATE_DOWN, b"no network"),
    lsd(IFT_PPP, LINK_STATE_DOWN, b"no carrier"),
    lsd(IFT_CARP, LINK_STATE_DOWN, b"backup"),
    lsd(IFT_CARP, LINK_STATE_UP, b"master"),
    lsd(IFT_CARP, LINK_STATE_HALF_DUPLEX, b"master"),
    lsd(IFT_CARP, LINK_STATE_FULL_DUPLEX, b"master"),
    lsd(0, LINK_STATE_UP, b"active"),
    lsd(0, LINK_STATE_HALF_DUPLEX, b"active"),
    lsd(0, LINK_STATE_FULL_DUPLEX, b"active"),
    lsd(0, LINK_STATE_UNKNOWN, b"unknown"),
    lsd(0, LINK_STATE_INVALID, b"invalid"),
    lsd(0, LINK_STATE_DOWN, b"down"),
    lsd(0, LINK_STATE_KALIVE_DOWN, b"keepalive down"),
];

const fn lsd(ifs_type: u8, ifs_state: u8, ifs_string: &'static [u8]) -> IfStatusDescription {
    IfStatusDescription {
        ifs_type,
        ifs_state,
        ifs_string,
    }
}

/// `NET_TASKQ`: the softnet task queues; one without `MULTIPROCESSOR`.
pub const NET_TASKQ: usize = 1;

/// `NBBY` (`<sys/select.h>`, not ported): bits per byte, for the index bitmap.
const NBBY: usize = u8::BITS as usize;

/// `struct if_idxmap`: the interface index map. The kernel maintains a mapping of interface
/// indexes to `struct ifnet` pointers: an array whose slot 0 stores the array's length
/// (interface index 0 is reserved and means "no interface", so `if_get(0)` is NULL). As
/// interfaces are attached the map is grown on demand, up to `USHRT_MAX` entries.
struct IfIdxmap {
    /// `serial`: the next index to try. Protected by: `lock`.
    serial: Cell<u32>,
    /// `count`: interfaces in the map. Protected by: `lock`.
    count: Cell<u32>,
    /// `map`: the array; written under `lock`, read without it (SMR protected in C).
    map: AtomicPtr<AtomicPtr<Ifnet>>,
    /// `lock`.
    lock: Rwlock,
    /// `usedidx`: bitmap of indices in use. Protected by: `lock`.
    usedidx: Cell<*mut u8>,
}

// SAFETY: the cells are touched only with `lock` held; `map` and its slots are atomics.
unsafe impl Sync for IfIdxmap {}

/// `struct softnet`: a softnet task queue and the packet lists its thread works through.
#[repr(align(64))]
pub struct Softnet {
    /// `sn_name`: the queue's name, written once by `softnet_init`.
    sn_name: UnsafeCell<[u8; 16]>,
    /// `sn_taskq`.
    pub sn_taskq: Cell<Option<&'static Taskq>>,
    /// `sn_netstack`: used only by the queue's thread (`if_input_process`).
    pub sn_netstack: Netstack,
}

// SAFETY: `sn_name` and `sn_taskq` are written once by `softnet_init` during `main`, before
// any packet moves; `sn_netstack` is used only by the softnet thread that owns it, one at a
// time (one thread per queue).
unsafe impl Sync for Softnet {}

impl Softnet {
    /// An empty softnet, before `softnet_init`.
    const fn new() -> Self {
        Self {
            sn_name: UnsafeCell::new([0; 16]),
            sn_taskq: Cell::new(None),
            sn_netstack: Netstack::new(),
        }
    }
}

/// `TAILQ_HEAD(, ifg_group)`, made `Sync`: `ifg_head` is protected by the net lock.
pub struct IfgHeadList(pub TailqHead<IfgHead>);

// SAFETY: see the type's doc.
unsafe impl Sync for IfgHeadList {}

/// `LIST_HEAD(, if_clone)`, made `Sync`: `if_cloners` is written only while `main` attaches
/// the pseudo-devices and is immutable afterwards.
pub struct IfClonersList(pub ListHead<IfClones>);

// SAFETY: see the type's doc.
unsafe impl Sync for IfClonersList {}

/// `ifg_head`: \[N\] list of interface groups.
pub static IFG_HEAD: IfgHeadList = IfgHeadList(TailqHead::new());

/// `if_cloners`: \[I\] list of clonable interfaces.
pub static IF_CLONERS: IfClonersList = IfClonersList(ListHead::new());
/// `if_cloners_count`: \[I\] number of clonable interfaces.
pub static IF_CLONERS_COUNT: AtomicI32 = AtomicI32::new(0);

/// `if_cloners_lock`.
pub static IF_CLONERS_LOCK: Rwlock = Rwlock::new("clonelk");
/// `if_tmplist_lock`.
pub static IF_TMPLIST_LOCK: Rwlock = Rwlock::new("iftmplk");

/// `if_hooks_mtx`: hooks should only be added, deleted, and run from a process context.
static IF_HOOKS_MTX: Mutex = Mutex::new(IPL_NONE);

/// `ifq_congestion`: the `ticks` of the last queue congestion.
pub static IFQ_CONGESTION: AtomicI32 = AtomicI32::new(0);
/// `netisr`: scheduling bits for network (`NETISR_*`).
pub static NETISR: AtomicI32 = AtomicI32::new(0);

/// `softnets`.
static SOFTNETS: [Softnet; NET_TASKQ] = [const { Softnet::new() }; NET_TASKQ];

/// `if_input_task_locked`: `if_netisr`, which `schednetisr` queues.
pub static IF_INPUT_TASK_LOCKED: Task = Task::new(if_netisr, ptr::null_mut());

/// `netlock`: serialize socket operations to ensure no new sleeping points are introduced in
/// IP output paths.
pub static NETLOCK: Rwlock = Rwlock::new_trace("netlock", DT_RWLOCK_IDX_NETLOCK);

/// `if_idxmap`.
static IF_IDXMAP: IfIdxmap = IfIdxmap {
    serial: Cell::new(0),
    count: Cell::new(0),
    map: AtomicPtr::new(ptr::null_mut()),
    lock: Rwlock::new("idxmaplk"),
    usedidx: Cell::new(ptr::null_mut()),
};

/// `ifnetlist`: all the interfaces. For modification both the kernel and the net lock should
/// be taken; for read-only access one of them is enough.
pub static IFNETLIST: IfnetHead = IfnetHead(TailqHead::new());

/// `softnet_count`'s `nsoftnets`.
static NSOFTNETS: AtomicU32 = AtomicU32::new(0);

/// `IFQ_PRIO2TOS(p)`: a queue priority as the precedence bits of an IP type of service.
pub const fn ifq_prio2tos(p: u32) -> u32 {
    p << 5
}

/// `IFQ_TOS2PRIO(t)`: the queue priority of an IP type of service.
pub const fn ifq_tos2prio(t: u32) -> u32 {
    t >> 5
}

/// `LINK_STATE_IS_UP(s)`: whether link state `s` counts as up (unknown does).
pub const fn link_state_is_up(s: u8) -> bool {
    s >= LINK_STATE_UP || s == LINK_STATE_UNKNOWN
}

/// `LINK_STATE_DESC_MATCH(ifs, t, s)`: whether description `ifs` applies to type `t` in state
/// `s`.
pub const fn link_state_desc_match(ifs: &IfStatusDescription, t: u8, s: u8) -> bool {
    (ifs.ifs_type == t || ifs.ifs_type == 0) && ifs.ifs_state == s
}

/// `IF_Kbps(x)`: kilobits/sec.
pub const fn if_kbps(x: u64) -> u64 {
    x * 1000
}

/// `IF_Mbps(x)`: megabits/sec.
pub const fn if_mbps(x: u64) -> u64 {
    if_kbps(x * 1000)
}

/// `IF_Gbps(x)`: gigabits/sec.
pub const fn if_gbps(x: u64) -> u64 {
    if_mbps(x * 1000)
}

/// The bytes of a C string up to its first NUL (or all of them).
fn cstr(s: &[u8]) -> &[u8] {
    &s[..strnlen(s, s.len())]
}

/// `strcmp(a, b) == 0` over two NUL-terminated (or slice-terminated) names.
fn name_eq(a: &[u8], b: &[u8]) -> bool {
    cstr(a) == cstr(b)
}

/// The bytes of `*v`, for `copyout` of a structure.
///
/// # Safety
///
/// Every byte of `*v` is initialised: `T` has no padding, or `*v` was zero-filled and then
/// written member by member (as `Ifreq::zeroed` and `MaybeUninit::zeroed` values are).
unsafe fn bytes_of<T>(v: &T) -> &[u8] {
    // SAFETY: `v` is `size_of::<T>()` readable bytes, initialised per the caller.
    unsafe { slice::from_raw_parts(ptr::from_ref(v).cast::<u8>(), size_of::<T>()) }
}

/// `(*ifp->if_ioctl)(ifp, cmd, data)`.
///
/// # Safety
///
/// As for [`IfIoctlFn`].
pub unsafe fn ifp_ioctl(ifp: &'static Ifnet, cmd: u64, data: *mut u8) -> Result<(), Errno> {
    match ifp.if_ioctl.get() {
        // SAFETY: the caller's contract is the hook's.
        Some(ioctl) => unsafe { ioctl(ifp, cmd, data) },
        None => panic(format_args!("{}: no if_ioctl", Str(&ifp.if_xname.get()))),
    }
}

/// `(*ifp->if_input)(ifp, m, ns)`.
fn ifp_input(ifp: &'static Ifnet, m: &'static Mbuf, ns: Option<&Netstack>) {
    match ifp.if_input.get() {
        Some(input) => input(ifp, m, ns),
        None => panic(format_args!("{}: no if_input", Str(&ifp.if_xname.get()))),
    }
}

/// `ifp->if_counters`, which the caller knows `if_counters_alloc` set.
fn ifp_counters(ifp: &Ifnet) -> &'static IfCounterArray {
    match ifp.if_counters.get() {
        Some(c) => c,
        None => panic(format_args!("{}: no if_counters", Str(&ifp.if_xname.get()))),
    }
}

/// `counters_inc(c, i)` (`<sys/percpu.h>`) over an interface's counters.
pub fn counters_inc(c: &IfCounterArray, i: IfCounters) {
    c[i as usize].fetch_add(1, Ordering::Relaxed);
}

/// `counters_pkt(c, pc, bc, v)`: one packet of `v` bytes.
pub fn counters_pkt(c: &IfCounterArray, pc: IfCounters, bc: IfCounters, v: u64) {
    c[pc as usize].fetch_add(1, Ordering::Relaxed);
    c[bc as usize].fetch_add(v, Ordering::Relaxed);
}

/// `ifinit`: network interface utility routines. Most machines boot with 4 or 5 interfaces,
/// so size the initial map to accommodate this.
pub fn ifinit() {
    if_idxmap_init(8); // 8 is a nice power of 2 for malloc
}

/// `softnet_init`: the softnet task queues. The number of CPUs is unknown, but driver attach
/// needs softnet tasks.
pub fn softnet_init() {
    for (i, sn) in SOFTNETS.iter().enumerate() {
        // SAFETY: written once, here, during `main` on one CPU, before the queue that names
        // it exists; never written again.
        let name: &'static mut [u8; 16] = unsafe { &mut *sn.sn_name.get() };
        let _ = snprintf(name, format_args!("softnet{i}"));
        let name: &'static [u8] = cstr(name);
        let tq = taskq_create(name, 1, IPL_NET, TASKQ_MPSAFE);
        if tq.is_none() {
            panic(format_args!("unable to create network taskq {i}"));
        }
        sn.sn_taskq.set(tq);
    }
}

/// `softnet_percpu`: after attaching all CPUs and interfaces, remove useless threads. Only
/// `MULTIPROCESSOR` kernels have more than one softnet.
pub fn softnet_percpu() {
    // MULTIPROCESSOR: taskq_destroy the softnets from softnet_count() on; not configured.
}

/// `if_idxmap_limit(if_map)`: the map's length, kept in slot 0.
///
/// # Safety
///
/// `if_map` is a live map made by `if_idxmap_init` or `if_idxmap_alloc`.
unsafe fn if_idxmap_limit(if_map: *const AtomicPtr<Ifnet>) -> u32 {
    // SAFETY: slot 0 of a live map holds its length.
    unsafe { (*if_map).load(Ordering::Relaxed) as usize as u32 }
}

/// `if_idxmap_usedidx_size(limit)`: the bytes of the bitmap for `limit` indices.
fn if_idxmap_usedidx_size(limit: u32) -> usize {
    howmany(limit as usize, NBBY)
}

/// The used-index bitmap of a map of `limit` slots.
///
/// # Safety
///
/// `IF_IDXMAP.lock` is held for writing and `limit` is the current map's.
unsafe fn if_idxmap_usedidx<'a>(limit: u32) -> &'a mut [u8] {
    // SAFETY: `usedidx` is the current map's bitmap of `if_idxmap_usedidx_size(limit)` bytes,
    // only touched under the lock the caller holds.
    unsafe { slice::from_raw_parts_mut(IF_IDXMAP.usedidx.get(), if_idxmap_usedidx_size(limit)) }
}

/// A new map of `limit` empty slots (slot 0 holding `limit`).
fn if_idxmap_newmap(limit: u32) -> *mut AtomicPtr<Ifnet> {
    let Some(if_map) = mallocarray(
        limit as usize,
        size_of::<AtomicPtr<Ifnet>>(),
        M_IFADDR,
        M_WAITOK | M_ZERO,
    ) else {
        panic(format_args!("if_idxmap: out of memory"));
    };
    let if_map = if_map.as_ptr().cast::<AtomicPtr<Ifnet>>();
    // SAFETY: a zero-filled array of `limit` (at least 1) slots; zero is a null pointer.
    unsafe {
        (*if_map).store(
            ptr::without_provenance_mut(limit as usize),
            Ordering::Relaxed,
        )
    };
    if_map
}

/// A zero-filled bitmap for `limit` indices.
fn if_idxmap_newusedidx(limit: u32) -> *mut u8 {
    match malloc(if_idxmap_usedidx_size(limit), M_IFADDR, M_WAITOK | M_ZERO) {
        Some(p) => p.as_ptr(),
        None => panic(format_args!("if_idxmap: out of memory")),
    }
}

/// `if_idxmap_init`: the first map, `limit` slots.
pub fn if_idxmap_init(limit: u32) {
    rw_init(&IF_IDXMAP.lock, "idxmaplk");
    IF_IDXMAP.serial.set(1); // skip ifidx 0

    let if_map = if_idxmap_newmap(limit);

    IF_IDXMAP.usedidx.set(if_idxmap_newusedidx(limit));
    // SAFETY: this is called early so there's nothing to race with; the bitmap was just made
    // for `limit` indices.
    setbit(unsafe { if_idxmap_usedidx(limit) }, 0); // blacklist ifidx 0

    // this is called early so there's nothing to race with
    IF_IDXMAP.map.store(if_map, Ordering::Release);
}

/// `if_idxmap_alloc`: picks the interface's index (`if_index`), growing the map if needed.
pub fn if_idxmap_alloc(ifp: &Ifnet) {
    refcnt_init(&ifp.if_refcnt);

    rw_enter_write(&IF_IDXMAP.lock);

    IF_IDXMAP.count.set(IF_IDXMAP.count.get() + 1);
    if IF_IDXMAP.count.get() >= u32::from(USHRT_MAX) {
        panic(format_args!("too many interfaces"));
    }

    let mut if_map = IF_IDXMAP.map.load(Ordering::Relaxed);
    // SAFETY: the current map, under the lock.
    let mut limit = unsafe { if_idxmap_limit(if_map) };

    let mut index = next_serial();

    if index >= limit {
        let oif_map = if_map;
        let olimit = limit;

        limit = olimit * 2;
        if_map = if_idxmap_newmap(limit);

        for i in 1..olimit as usize {
            // SAFETY: `i` is below the old map's length.
            let oifp = unsafe { (*oif_map.add(i)).load(Ordering::Relaxed) };
            // SAFETY: a map slot holds NULL or an interface the map holds a reference to.
            let Some(oifp) = (unsafe { oifp.as_ref() }) else {
                continue;
            };

            // nif_map isn't visible yet, so don't need SMR_PTR_SET_LOCKED and its membar.
            // SAFETY: `i` is below the new, larger map's length.
            unsafe {
                (*if_map.add(i)).store(ptr::from_ref(if_ref(oifp)).cast_mut(), Ordering::Relaxed)
            };
        }

        let nusedidx = if_idxmap_newusedidx(limit);
        let ousedidx = IF_IDXMAP.usedidx.get();
        // SAFETY: both bitmaps hold at least the old map's bytes.
        unsafe {
            ptr::copy_nonoverlapping(ousedidx, nusedidx, howmany(olimit as usize, NBBY));
        }

        IF_IDXMAP.usedidx.set(nusedidx);

        IF_IDXMAP.map.store(if_map, Ordering::Release);

        // smr_call(&dtor->smr, if_idxmap_free, dtor): no SMR, see the module's deviations;
        // the old bitmap was the C's smr_entry.
        // SAFETY: the old map is no longer published, and no reader can still be inside it.
        unsafe { if_idxmap_free(oif_map) };
        if let Some(ousedidx) = NonNull::new(ousedidx) {
            free(ousedidx, M_IFADDR, if_idxmap_usedidx_size(olimit));
        }
    }

    // pick the next free index
    // SAFETY: the current map's bitmap, under the lock.
    let usedidx = unsafe { if_idxmap_usedidx(limit) };
    for _ in 0..USHRT_MAX {
        if index != 0 && isclr(usedidx, index as usize) {
            break;
        }

        index = next_serial();
    }
    kassert!(index != 0 && index < limit);
    kassert!(isclr(usedidx, index as usize));

    setbit(usedidx, index as usize);
    ifp.if_index.set(index);

    rw_exit_write(&IF_IDXMAP.lock);
}

/// `if_idxmap.serial++ & USHRT_MAX` (the lock held).
fn next_serial() -> u32 {
    let serial = IF_IDXMAP.serial.get();
    IF_IDXMAP.serial.set(serial.wrapping_add(1));
    serial & u32::from(USHRT_MAX)
}

/// `if_idxmap_free`: drops the references of an old map and frees it.
///
/// # Safety
///
/// `oif_map` is a map that is no longer published and that no reader is inside.
unsafe fn if_idxmap_free(oif_map: *mut AtomicPtr<Ifnet>) {
    // SAFETY: the caller's map is live until freed below.
    let olimit = unsafe { if_idxmap_limit(oif_map) };

    for i in 1..olimit as usize {
        // SAFETY: `i` is below the map's length; a slot is NULL or holds a reference.
        let oifp = unsafe { (*oif_map.add(i)).load(Ordering::Relaxed).as_ref() };
        if_put(oifp);
    }

    if let Some(oif_map) = NonNull::new(oif_map.cast::<u8>()) {
        free(
            oif_map,
            M_IFADDR,
            olimit as usize * size_of::<AtomicPtr<Ifnet>>(),
        );
    }
}

/// `if_idxmap_insert`: publishes the interface at its index.
pub fn if_idxmap_insert(ifp: &'static Ifnet) {
    let index = ifp.if_index.get();

    rw_enter_write(&IF_IDXMAP.lock);

    let if_map = IF_IDXMAP.map.load(Ordering::Relaxed);
    // SAFETY: the current map, under the lock.
    let limit = unsafe { if_idxmap_limit(if_map) };

    if index == 0 || index >= limit {
        panic(format_args!(
            "{}({:p}) index {index} vs limit {limit}",
            Str(&ifp.if_xname.get()),
            ifp
        ));
    }
    // SAFETY: `index` is below the map's length.
    let slot = unsafe { &*if_map.add(index as usize) };
    kassert!(slot.load(Ordering::Relaxed).is_null());
    // SAFETY: the current map's bitmap, under the lock.
    kassert!(isset(unsafe { if_idxmap_usedidx(limit) }, index as usize));

    // commit
    slot.store(ptr::from_ref(if_ref(ifp)).cast_mut(), Ordering::Release);

    rw_exit_write(&IF_IDXMAP.lock);
}

/// `if_idxmap_remove`: takes the interface out of the map and frees its index.
pub fn if_idxmap_remove(ifp: &'static Ifnet) {
    let index = ifp.if_index.get();

    rw_enter_write(&IF_IDXMAP.lock);

    let if_map = IF_IDXMAP.map.load(Ordering::Relaxed);
    // SAFETY: the current map, under the lock.
    let limit = unsafe { if_idxmap_limit(if_map) };

    kassert!(index != 0 && index < limit);
    // SAFETY: `index` was checked against the length by `if_idxmap_insert`.
    let slot = unsafe { &*if_map.add(index as usize) };
    kassert!(ptr::eq(slot.load(Ordering::Relaxed), ifp));
    // SAFETY: the current map's bitmap, under the lock.
    let usedidx = unsafe { if_idxmap_usedidx(limit) };
    kassert!(isset(usedidx, index as usize));

    slot.store(ptr::null_mut(), Ordering::Release);

    IF_IDXMAP.count.set(IF_IDXMAP.count.get() - 1);
    clrbit(usedidx, index as usize);
    // end of if_idxmap modifications

    rw_exit_write(&IF_IDXMAP.lock);

    // smr_barrier(): no SMR (see the module's deviations).
    if_put(ifp);
}

/// `if_idxmap_get`: the interface at `index`, without taking a reference (SMR read section in
/// C).
fn if_idxmap_get(index: u32) -> Option<&'static Ifnet> {
    if index == 0 {
        return None;
    }

    let if_map = IF_IDXMAP.map.load(Ordering::Acquire);
    if if_map.is_null() {
        return None;
    }
    // SAFETY: a published map stays valid while no writer replaces it, which on one CPU
    // cannot happen during this read (see the module's deviations).
    if index < unsafe { if_idxmap_limit(if_map) } {
        // SAFETY: `index` is below the map's length; a slot is NULL or an interface the map
        // holds a reference to.
        unsafe {
            (*if_map.add(index as usize))
                .load(Ordering::Acquire)
                .as_ref()
        }
    } else {
        None
    }
}

/// `if_attachsetup`: attach an interface to the list of "active" interfaces.
fn if_attachsetup(ifp: &'static Ifnet) {
    net_assert_locked("if_attachsetup");

    let _ = if_addgroup(ifp, IFG_ALL);

    #[cfg(feature = "inet6")]
    nd6_ifattach(ifp);

    pfi_attach_ifnet(ifp);

    timeout_set(
        &ifp.if_slowtimo,
        if_slowtimo,
        ptr::from_ref(ifp).cast_mut().cast(),
    );
    if_slowtimo(ptr::from_ref(ifp).cast_mut().cast());

    if_idxmap_insert(ifp);
    kassert!(if_get(0).is_none());

    let ifidx = ifp.if_index.get() as usize;

    task_set(
        &ifp.if_watchdogtask,
        if_watchdog_task,
        ptr::without_provenance_mut(ifidx),
    );
    task_set(
        &ifp.if_linkstatetask,
        if_linkstate_task,
        ptr::without_provenance_mut(ifidx),
    );

    // Announce the interface.
    rtm_ifannounce(ifp, IFAN_ARRIVAL);
}

/// `if_alloc_sadl`: allocate the link level name for the specified interface. This is an
/// attachment helper. It must be called after `ifp->if_addrlen` is initialized, which may not
/// be the case when `if_attach()` is called.
pub fn if_alloc_sadl(ifp: &Ifnet) {
    // If the interface already has a link name, release it now. This is useful for
    // interfaces that can change link types, and thus switch link names often.
    if_free_sadl(ifp);

    let xname = ifp.if_xname.get();
    let name = cstr(&xname);
    let namelen = name.len();
    let masklen = offset_of!(SockaddrDl, sdl_data) + namelen;
    let mut socksize = masklen + usize::from(ifp.if_addrlen.get());
    // ROUNDUP(a): to a multiple of sizeof(long).
    let roundup = |a: usize| 1 + ((a - 1) | (size_of::<u64>() - 1));
    if socksize < size_of::<SockaddrDl>() {
        socksize = size_of::<SockaddrDl>();
    }
    socksize = roundup(socksize);
    let Some(sdl) = malloc(socksize, M_IFADDR, M_WAITOK | M_ZERO) else {
        panic(format_args!("if_alloc_sadl: out of memory"));
    };
    let sdl = sdl.as_ptr().cast::<SockaddrDl>();
    // SAFETY: a zero-filled allocation of `socksize` bytes, at least a `sockaddr_dl` and
    // aligned for it (malloc's chunks are aligned to their size); the name fits since
    // `socksize` counts it after `sdl_data`'s offset.
    unsafe {
        (*sdl).sdl_len = socksize as u8;
        (*sdl).sdl_family = AF_LINK;
        ptr::copy_nonoverlapping(
            name.as_ptr(),
            ptr::addr_of_mut!((*sdl).sdl_data).cast::<u8>(),
            namelen,
        );
        (*sdl).sdl_nlen = namelen as u8;
        (*sdl).sdl_alen = ifp.if_addrlen.get();
        (*sdl).sdl_index = ifp.if_index.get() as u16;
        (*sdl).sdl_type = ifp.if_type.get();
    }
    ifp.if_sadl.set(sdl);
}

/// `if_free_sadl`: free the link level name for the specified interface. This is a detach
/// helper. This is called from `if_detach()` or from link layer type specific detach
/// functions.
pub fn if_free_sadl(ifp: &Ifnet) {
    let Some(sdl) = NonNull::new(ifp.if_sadl.get()) else {
        return;
    };

    // SAFETY: `if_sadl` is the allocation `if_alloc_sadl` made, `sdl_len` bytes.
    let len = usize::from(unsafe { sdl.as_ref().sdl_len });
    free(sdl.cast(), M_IFADDR, len);
    ifp.if_sadl.set(ptr::null_mut());
}

/// The check that makes `if_attach` safe: an interface is attached once.
fn if_attach_check(ifp: &Ifnet) {
    if ifp.if_index.get() != 0 {
        panic(format_args!(
            "{}: attached twice (index {})",
            Str(&ifp.if_xname.get()),
            ifp.if_index.get()
        ));
    }
}

/// `if_attachhead`: as `if_attach`, at the head of `ifnetlist` (`lo0`).
pub fn if_attachhead(ifp: &'static Ifnet) {
    if_attach_check(ifp);
    if_attach_common(ifp);
    net_lock();
    // SAFETY: a new interface (no index before `if_attach_common`) is on no list; it lives as
    // long as it is attached.
    unsafe { IFNETLIST.0.insert_head(ifp) };
    if_attachsetup(ifp);
    net_unlock();
}

/// `if_attach`: attaches a network interface (a driver's `struct ifnet`, filled in with its
/// name, `if_softc`, flags, `if_ioctl` and `if_start`/`if_qstart`) to the system.
pub fn if_attach(ifp: &'static Ifnet) {
    if_attach_check(ifp);
    if_attach_common(ifp);
    net_lock();
    // SAFETY: as in `if_attachhead`.
    unsafe { IFNETLIST.0.insert_tail(ifp) };
    if_attachsetup(ifp);
    net_unlock();
}

/// `if_attach_queues`: gives the interface `nqs` send queues (one per transmit ring), after
/// `if_attach`.
pub fn if_attach_queues(ifp: &'static Ifnet, nqs: u32) {
    kassert!(ptr::eq(ifp.if_ifqs.get(), ifp.if_snd.ifq_ifqs.as_ptr()));
    kassert!(nqs != 0);

    let Some(map) = mallocarray(
        nqs as usize,
        size_of::<Cell<*const Ifqueue>>(),
        M_DEVBUF,
        M_WAITOK,
    ) else {
        panic(format_args!("if_attach_queues: out of memory"));
    };
    let map = map.as_ptr().cast::<Cell<*const Ifqueue>>();

    ifp.if_snd.ifq_softc.set(ptr::null_mut());
    // SAFETY: `map` has `nqs` slots, each written before `if_ifqs` publishes it.
    unsafe { map.write(Cell::new(ptr::from_ref(&ifp.if_snd))) };

    for i in 1..nqs {
        let Some(ifq) = malloc(size_of::<Ifqueue>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
            panic(format_args!("if_attach_queues: out of memory"));
        };
        // SAFETY: a zero-filled `Ifqueue`-sized block, aligned for it; the all-zero queue is
        // valid; it lives until `if_detach` frees it.
        let ifq: &'static Ifqueue = unsafe { &*ifq.as_ptr().cast::<Ifqueue>() };
        ifq_init_maxlen(ifq, ifp.if_snd.ifq_maxlen.get());
        ifq_init(ifq, ifp, i);
        // SAFETY: slot `i` is below `nqs`.
        unsafe { map.add(i as usize).write(Cell::new(ptr::from_ref(ifq))) };
    }

    ifp.if_ifqs.set(map);
    ifp.if_nifqs.set(nqs);
}

/// `if_attach_iqueues`: gives the interface `niqs` input queues, after `if_attach`.
pub fn if_attach_iqueues(ifp: &'static Ifnet, niqs: u32) {
    kassert!(niqs != 0);

    let Some(map) = mallocarray(
        niqs as usize,
        size_of::<Cell<*const Ifiqueue>>(),
        M_DEVBUF,
        M_WAITOK,
    ) else {
        panic(format_args!("if_attach_iqueues: out of memory"));
    };
    let map = map.as_ptr().cast::<Cell<*const Ifiqueue>>();

    ifp.if_rcv.ifiq_softc.set(ptr::null_mut());
    // SAFETY: `map` has `niqs` slots, each written before `if_iqs` publishes it.
    unsafe { map.write(Cell::new(ptr::from_ref(&ifp.if_rcv))) };

    for i in 1..niqs {
        let Some(ifiq) = malloc(size_of::<Ifiqueue>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
            panic(format_args!("if_attach_iqueues: out of memory"));
        };
        // SAFETY: as for the send queues of `if_attach_queues`.
        let ifiq: &'static Ifiqueue = unsafe { &*ifiq.as_ptr().cast::<Ifiqueue>() };
        ifiq_init(ifiq, ifp, i);
        // SAFETY: slot `i` is below `niqs`.
        unsafe { map.add(i as usize).write(Cell::new(ptr::from_ref(ifiq))) };
    }

    ifp.if_iqs.set(map);
    ifp.if_niqs.set(niqs);
}

/// `if_attach_common`: what `if_attach` and `if_attachhead` share.
fn if_attach_common(ifp: &'static Ifnet) {
    kassert!(ifp.if_ioctl.get().is_some());

    ifp.if_addrlist.init();
    ifp.if_maddrlist.init();
    ifp.if_groups.init();
    rw_init(&ifp.if_maddrlock, "maddr");

    if ifp.if_xflags.get() & IFXF_MPSAFE == 0 {
        // KASSERTMSG: "%s: if_qstart set without MPSAFE set".
        kassert!(ifp.if_qstart.get().is_none());
        ifp.if_qstart.set(Some(if_qstart_compat));
    } else {
        // KASSERTMSG: "%s: if_start set with MPSAFE set".
        kassert!(ifp.if_start.get().is_none());
        // KASSERTMSG: "%s: if_qstart not set with MPSAFE set".
        kassert!(ifp.if_qstart.get().is_some());
    }

    if_idxmap_alloc(ifp);

    ifq_init(&ifp.if_snd, ifp, 0);

    ifp.if_snd.ifq_ifqs[0].set(&ifp.if_snd);
    ifp.if_ifqs.set(ifp.if_snd.ifq_ifqs.as_ptr());
    ifp.if_nifqs.set(1);
    if ifp.if_txmit.get() == 0 {
        ifp.if_txmit.set(IF_TXMIT_DEFAULT);
    }

    ifiq_init(&ifp.if_rcv, ifp, 0);

    ifp.if_rcv.ifiq_ifiqs[0].set(&ifp.if_rcv);
    ifp.if_iqs.set(ifp.if_rcv.ifiq_ifiqs.as_ptr());
    ifp.if_niqs.set(1);

    ifp.if_addrhooks.init();
    ifp.if_linkstatehooks.init();
    ifp.if_detachhooks.init();

    if ifp.if_rtrequest.get().is_none() {
        ifp.if_rtrequest.set(Some(if_rtrequest_dummy));
    }
    if ifp.if_enqueue.get().is_none() {
        ifp.if_enqueue.set(Some(if_enqueue_ifq));
    }
    if ifp.if_bpf_mtap.get().is_none() {
        ifp.if_bpf_mtap.set(Some(bpf_mtap_ether));
    }
    ifp.if_llprio.set(IFQ_DEFPRIO as u8);
}

/// `if_attach_ifq`: only switch the `ifq_ops` on the first ifq on an interface. The only
/// `ifq_ops` we provide are priq and hfsc, and hfsc only works on a single ifq. Because the
/// code uses the `ifq_ops` on the first ifq (`if_snd`) to select a queue for an mbuf, by
/// switching only the first one we change both the algorithm and force the routing of all new
/// packets to it.
pub fn if_attach_ifq(ifp: &Ifnet, newops: &'static IfqOps, args: *mut c_void) {
    ifq_attach(&ifp.if_snd, newops, args);
}

/// `if_start`: runs the start routine of a driver that is not `IFXF_MPSAFE`.
pub fn if_start(ifp: &'static Ifnet) {
    // KASSERT(ifp->if_qstart == if_qstart_compat): function pointers do not compare reliably
    // in Rust; a driver without IFXF_MPSAFE is what makes if_attach_common install it.
    kassert!(ifp.if_xflags.get() & IFXF_MPSAFE == 0);
    if_qstart_compat(&ifp.if_snd);
}

/// `if_qstart_compat`: the stack assumes that an interface can have multiple transmit
/// rings, but a lot of drivers are still written so that interfaces and send rings have a 1:1
/// mapping. This provides compatibility between the stack and the older drivers by
/// translating from the only queue they have (`ifp->if_snd`) back to the interface and
/// calling `if_start`.
pub fn if_qstart_compat(ifq: &'static Ifqueue) {
    let Some(ifp) = ifq.ifq_if.get() else {
        return;
    };

    // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
    let s = splnet();
    if let Some(start) = ifp.if_start.get() {
        start(ifp);
    }
    splx(s);
    // KERNEL_UNLOCK()
}

/// `if_enqueue`: hands a finished packet to the interface for transmission.
pub fn if_enqueue(ifp: &'static Ifnet, m: &'static Mbuf) -> Result<(), Errno> {
    let ph = m.m_pkthdr();
    ph.csum_flags.set(ph.csum_flags.get() & !M_TIMESTAMP);

    if m.m_pkthdr().pf.delay.get() > 0 {
        return pf_delay_pkt(m, ifp.if_index.get());
    }

    // NBRIDGE > 0: a bridge port (if_bridgeidx) hands the packet to bridge_enqueue unless it
    // has M_PROTO1; bridge(4) is not configured.

    pf_pkt_addr_changed(m);

    match ifp.if_enqueue.get() {
        Some(enqueue) => enqueue(ifp, m),
        None => panic(format_args!("{}: no if_enqueue", Str(&ifp.if_xname.get()))),
    }
}

/// `if_enqueue_ifq`: the default `if_enqueue`: queues the packet on one of the interface's
/// send queues and starts it.
pub fn if_enqueue_ifq(ifp: &'static Ifnet, m: &'static Mbuf) -> Result<(), Errno> {
    let mut ifq: &'static Ifqueue = &ifp.if_snd;

    if ifp.if_nifqs.get() > 1 {
        // use the operations on the first ifq to pick which of the array gets this mbuf.
        let idx = ifq_idx(&ifp.if_snd, ifp.if_nifqs.get(), m);
        ifq = ifp.ifq(idx);
    }

    ifq_enqueue(ifq, m)?;

    ifq_start(ifq);

    Ok(())
}

/// `if_input`: a driver's received packets.
pub fn if_input(ifp: &'static Ifnet, ml: &MbufList) {
    let _ = ifiq_input(&ifp.if_rcv, ml);
}

/// `if_input_local`: a packet looped back to this host (`lo(4)`, and copies of broadcasts on
/// simplex interfaces), handed to its protocol's input function.
pub fn if_input_local(
    ifp: &'static Ifnet,
    m: &'static Mbuf,
    af: SaFamily,
    ns: Option<&Netstack>,
) -> Result<(), Errno> {
    // Only send packets to bpf if they are destined to local addresses.
    //
    // if_input_local() is also called for SIMPLEX interfaces to duplicate packets for local
    // use. But don't dup them to bpf.
    if ifp.if_flags.get() & IFF_LOOPBACK != 0 {
        let if_bpf = ifp.if_bpf.get();

        if !if_bpf.is_null() {
            let _ = bpf_mtap_af(if_bpf, u32::from(af), m, BPF_DIRECTION_OUT);
        }
    }

    let keepflags = m.m_flags().get() & (M_BCAST | M_MCAST);
    // Preserve outgoing checksum flags, in case the packet is forwarded to another interface.
    // Then the checksum, which is now incorrect, will be calculated before sending.
    let ph = m.m_pkthdr();
    let keepcksum = ph.csum_flags.get()
        & (M_IPV4_CSUM_OUT
            | M_TCP_CSUM_OUT
            | M_UDP_CSUM_OUT
            | M_ICMP_CSUM_OUT
            | M_TCP_TSO
            | M_FLOWID);
    let keepmss = ph.ph_mss.get();
    let keepflowid = ph.ph_flowid.get();
    m_resethdr(m);
    m.m_flags().set(m.m_flags().get() | M_LOOP | keepflags);
    ph.csum_flags.set(keepcksum);
    ph.ph_mss.set(keepmss);
    ph.ph_flowid.set(keepflowid);
    ph.ph_ifidx.set(ifp.if_index.get());
    ph.ph_rtableid.set(ifp.if_rdomain.get());

    if keepcksum & M_TCP_TSO != 0 && ph.len.get() as u32 > ifp.if_mtu.get() {
        if ifp.if_mtu.get() > 0
            && ((af == AF_INET && ifp.if_capabilities.get() & IFCAP_TSOv4 != 0)
                || (af == AF_INET6 && ifp.if_capabilities.get() & IFCAP_TSOv6 != 0))
        {
            tcpstat_inc(TcpstatCounters::TcpsInhwlro);
        } else {
            tcpstat_inc(TcpstatCounters::TcpsInbadlro);
            m_freem(m);
            return Err(Errno::EPROTONOSUPPORT);
        }
    }

    if keepcksum & M_TCP_CSUM_OUT != 0 {
        ph.csum_flags.set(ph.csum_flags.get() | M_TCP_CSUM_IN_OK);
    }
    if keepcksum & M_UDP_CSUM_OUT != 0 {
        ph.csum_flags.set(ph.csum_flags.get() | M_UDP_CSUM_IN_OK);
    }
    if keepcksum & M_ICMP_CSUM_OUT != 0 {
        ph.csum_flags.set(ph.csum_flags.get() | M_ICMP_CSUM_IN_OK);
    }

    // do not count multicast loopback and simplex interfaces
    if ifp.if_flags.get() & IFF_LOOPBACK != 0 {
        counters_pkt(
            ifp_counters(ifp),
            IfCounters::IfcOpackets,
            IfCounters::IfcObytes,
            ph.len.get() as u64,
        );
    }

    match af {
        AF_INET => {
            if keepcksum & M_IPV4_CSUM_OUT != 0 {
                ph.csum_flags.set(ph.csum_flags.get() | M_IPV4_CSUM_IN_OK);
            }
            if_input_proto(ifp, m, ipv4_input, ns);
            Ok(())
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            if_input_proto(ifp, m, ipv6_input, ns);
            Ok(())
        }
        // MPLS: AF_MPLS goes to mpls_input; MPLS is not configured.
        _ => {
            printf(format_args!(
                "{}: can't handle af{}\n",
                Str(&ifp.if_xname.get()),
                af
            ));
            m_freem(m);
            Err(Errno::EAFNOSUPPORT)
        }
    }
}

/// `if_output_ml`: sends a list of packets through `if_output`; on the first error the rest
/// are freed.
///
/// # Safety
///
/// As for [`IfOutputFn`].
pub unsafe fn if_output_ml(
    ifp: &'static Ifnet,
    ml: &MbufList,
    dst: *const Sockaddr,
    rt: Option<&'static Rtentry>,
) -> Result<(), Errno> {
    let mut error = Ok(());

    while let Some(m) = ml_dequeue(ml) {
        // SAFETY: the caller's contract is `if_output`'s.
        error = unsafe { ifp_output(ifp, m, dst, rt) };
        if error.is_err() {
            break;
        }
    }
    if error.is_err() {
        let _ = ml_purge(ml);
    }

    error
}

/// `(*ifp->if_output)(ifp, m, dst, rt)`.
///
/// # Safety
///
/// As for [`IfOutputFn`].
unsafe fn ifp_output(
    ifp: &'static Ifnet,
    m: &'static Mbuf,
    dst: *const Sockaddr,
    rt: Option<&'static Rtentry>,
) -> Result<(), Errno> {
    match ifp.if_output.get() {
        // SAFETY: the caller's contract is the hook's.
        Some(output) => unsafe { output(ifp, m, dst, rt) },
        None => panic(format_args!("{}: no if_output", Str(&ifp.if_xname.get()))),
    }
}

/// `if_output_tso`: sends a TCP packet with TSO, or chops it, or sends it whole when it fits
/// `mtu`; `*mp` is left set when the packet still has to be fragmented or dropped.
///
/// # Safety
///
/// As for [`IfOutputFn`].
pub unsafe fn if_output_tso(
    ifp: &'static Ifnet,
    mp: &mut Option<&'static Mbuf>,
    dst: *const Sockaddr,
    rt: Option<&'static Rtentry>,
    mtu: u32,
) -> Result<(), Errno> {
    // SAFETY: `dst` is readable per the contract.
    let family = unsafe { (*dst).sa_family };
    let ifcap: u32 = match family {
        AF_INET => IFCAP_TSOv4,
        #[cfg(feature = "inet6")]
        AF_INET6 => IFCAP_TSOv6,
        _ => unhandled_af(i32::from(family)),
    };

    // Try to send with TSO first. When forwarding LRO may set maximum segment size in mbuf
    // header. Chop TCP segment even if it would fit interface MTU to preserve maximum path
    // MTU.
    // SAFETY: the caller's contract is `if_output`'s, which `tcp_if_output_tso` passes on.
    unsafe { tcp_if_output_tso(ifp, mp, dst, rt, ifcap, mtu) }?;
    let Some(m) = *mp else {
        return Ok(());
    };

    if m.m_pkthdr().len.get() as u32 <= mtu {
        match family {
            AF_INET => {
                in_hdr_cksum_out(m, Some(ifp));
                in_proto_cksum_out(m, Some(ifp));
            }
            #[cfg(feature = "inet6")]
            AF_INET6 => in6_proto_cksum_out(m, Some(ifp)),
            _ => {}
        }
        // SAFETY: the caller's contract is `if_output`'s.
        let error = unsafe { ifp_output(ifp, m, dst, rt) };
        *mp = None;
        return error;
    }

    // mp still contains mbuf that has to be fragmented or dropped.
    Ok(())
}

/// `if_output_mq`: sends the packets of a queue; `total` (the packets accounted to the queue
/// elsewhere) drops by what was sent and by what was discarded.
///
/// # Safety
///
/// As for [`IfOutputFn`].
pub unsafe fn if_output_mq(
    ifp: &'static Ifnet,
    mq: &MbufQueue,
    total: &AtomicU32,
    dst: *const Sockaddr,
    rt: Option<&'static Rtentry>,
) -> Result<(), Errno> {
    let ml = MbufList::new();

    mq_delist(mq, &ml);
    let len = ml_len(&ml);
    // SAFETY: the caller's contract.
    let error = unsafe { if_output_ml(ifp, &ml, dst, rt) };

    // XXXSMP we also discard if other CPU enqueues
    if mq_len(mq) > 0 {
        // mbuf is back in queue. Discard.
        total.fetch_sub(len + mq_purge(mq), Ordering::Relaxed);
    } else {
        total.fetch_sub(len, Ordering::Relaxed);
    }

    error
}

/// `if_output_local`: queues a packet for local delivery on `ifp`'s input queues (the
/// loopback's output), to be handed to `if_input_local` by the softnet task.
pub fn if_output_local(ifp: &'static Ifnet, m: &'static Mbuf, af: SaFamily) -> Result<(), Errno> {
    let mut flow = 0;

    m.m_pkthdr().ph_family.set(af);

    if m.m_pkthdr().csum_flags.get() & M_FLOWID != 0 {
        flow = u32::from(m.m_pkthdr().ph_flowid.get());
    }

    let ifiq = ifp.ifiq(flow % ifp.if_niqs.get());

    ifiq_enqueue_qlim(ifiq, m, 8192).map_err(|_| Errno::ENOBUFS)
}

/// `if_input_proto`: hands a packet to a protocol's input function: at once when the caller
/// holds the net lock (`ns` NULL), otherwise through the netstack's `ns_proto` list.
pub fn if_input_proto(
    ifp: &'static Ifnet,
    m: &'static Mbuf,
    input: IfInputFn,
    ns: Option<&Netstack>,
) {
    let Some(ns) = ns else {
        net_assert_locked("if_input_proto");
        input(ifp, m, None);
        return;
    };

    m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
    m.m_pkthdr().ph_cookie.set(input as *mut ());
    ml_enqueue(&ns.ns_proto, m);
}

/// `if_input_process`: the softnet thread's work on a list of received packets: each goes to
/// its interface's `if_input`, then to the protocols, with the shared net lock held.
pub fn if_input_process(ifp: &'static Ifnet, ml: &MbufList, idx: u32) {
    if ml_empty(ml) {
        return;
    }

    if ifp.if_xflags.get() & IFXF_CLONED == 0 {
        let first = mbuf_list_first(ml).map_or(0, |m| ptr::from_ref(m).addr());
        enqueue_randomness(ml_len(ml) ^ first as u32);
    }

    // We grab the shared netlock for packet processing in the softnet threads. Packets can
    // regrab the exclusive lock via queues. ioctl, sysctl, and socket syscall may use shared
    // lock if access is read only or MP safe. Usually they hold the exclusive net lock.

    let sn = net_sn(idx);
    let ns = &sn.sn_netstack;
    ml_init(&ns.ns_input);
    ml_init(&ns.ns_proto);

    ml_init(&ns.ns_tcp_ml);
    #[cfg(feature = "inet6")]
    ml_init(&ns.ns_tcp6_ml);

    net_lock_shared();
    while let Some(m) = ml_dequeue(ml) {
        ifp_input(ifp, m, Some(ns));
    }

    loop {
        while let Some(m) = ml_dequeue(&ns.ns_input) {
            // smr_read_enter()/smr_read_leave(): see the module's deviations.
            match if_idxmap_get(m.m_pkthdr().ph_ifidx.get()) {
                Some(ifp) => ifp_input(ifp, m, Some(ns)),
                None => {
                    m_freem(m);
                }
            }
        }

        while let Some(m) = ml_dequeue(&ns.ns_proto) {
            match if_idxmap_get(m.m_pkthdr().ph_ifidx.get()) {
                Some(ifp) => if_input_process_proto(ifp, m, Some(ns)),
                None => {
                    m_freem(m);
                }
            }
        }

        tcp_input_mlist(&ns.ns_tcp_ml, i32::from(AF_INET));
        #[cfg(feature = "inet6")]
        tcp_input_mlist(&ns.ns_tcp6_ml, i32::from(AF_INET6));

        if ml_empty(&ns.ns_input) {
            break;
        }
    }
    net_unlock_shared();
}

/// `if_vinput`: a virtual interface's input (`vlan(4)` and the like): the packet goes to
/// `ifp`'s input again through the netstack, or through its input queue without one.
pub fn if_vinput(ifp: &'static Ifnet, m: &'static Mbuf, ns: Option<&Netstack>) {
    pf_pkt_addr_changed(m);

    let Some(ns) = ns else {
        let _ = ifiq_enqueue(&ifp.if_rcv, m);
        return;
    };

    m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
    m.m_pkthdr().ph_rtableid.set(ifp.if_rdomain.get());

    counters_pkt(
        ifp_counters(ifp),
        IfCounters::IfcIpackets,
        IfCounters::IfcIbytes,
        m.m_pkthdr().len.get() as u64,
    );

    let if_bpf = ifp.if_bpf.get();
    if !if_bpf.is_null()
        && let Some(mtap) = ifp.if_bpf_mtap.get()
        && mtap(if_bpf, m, BPF_DIRECTION_IN)
    {
        m_freem(m);
        return;
    }

    if ifp.if_xflags.get() & IFXF_MONITOR == 0 {
        ml_enqueue(&ns.ns_input, m);
    } else {
        m_freem(m);
    }
}

/// `if_netisr`: the network software interrupt, as a task (`if_input_task_locked`): runs the
/// protocol queues whose `NETISR_*` bits `schednetisr` set.
fn if_netisr(_unused: *mut c_void) {
    net_lock();

    loop {
        let n = NETISR.load(Ordering::Relaxed);
        if n == 0 {
            break;
        }

        // Like sched_pause() but with a rwlock dance.
        if Machine::ci_schedstate(curcpu())
            .spc_schedflags
            .load(Ordering::Relaxed)
            & SPCF_SHOULDYIELD
            != 0
        {
            net_unlock();
            r#yield();
            net_lock();
        }

        NETISR.fetch_and(!n, Ordering::Relaxed);

        // NETHER > 0
        if n & (1 << NETISR_ARP) != 0 {
            arpintr();
        }
        if n & (1 << NETISR_IP) != 0 {
            ipintr();
        }
        #[cfg(feature = "inet6")]
        if n & (1 << NETISR_IPV6) != 0 {
            ip6intr();
        }
        // NPPP > 0: NETISR_PPP runs pppintr() under the kernel lock; ppp(4) is not configured.
        // NBRIDGE > 0: NETISR_BRIDGE runs bridgeintr(); bridge(4) is not configured.
        // NPPPOE > 0: NETISR_PPPOE runs pppoeintr(); pppoe(4) is not configured.
        // (t |= n: the C accumulates the bits it ran and never reads them.)
    }

    net_unlock();
}

/// `if_hooks_run`: runs a hook list, in order; a hook may add or delete hooks while it runs
/// (a cursor task without a function marks the position).
fn if_hooks_run(hooks: &TailqHead<TaskList>) {
    let cursor = Task::zeroed();

    mtx_enter(&IF_HOOKS_MTX);
    let mut t = hooks.first();
    while let Some(tt) = t {
        let Some(func) = tt.t_func.get() else {
            // skip cursors
            t = TailqHead::<TaskList>::next(tt);
            continue;
        };
        let arg = tt.t_arg.get();

        // SAFETY: `tt` is on `hooks` (under `IF_HOOKS_MTX`) and the cursor, on no list,
        // lives on this stack until it is removed below.
        unsafe { hooks.insert_after(tt, &cursor) };
        mtx_leave(&IF_HOOKS_MTX);

        func(arg);

        mtx_enter(&IF_HOOKS_MTX);
        t = TailqHead::<TaskList>::next(&cursor); // avoid _Q_INVALIDATE
        // SAFETY: the cursor is on `hooks`; only this function moves it.
        unsafe { hooks.remove(&cursor) };
    }
    mtx_leave(&IF_HOOKS_MTX);
}

/// `if_remove`: takes the interface off `ifnetlist` and the index map and waits for the
/// last reference to go.
fn if_remove(ifp: &'static Ifnet) {
    // Remove the interface from the list of all interfaces.
    net_lock();
    // SAFETY: an attached interface is on `ifnetlist`, under both locks.
    unsafe { IFNETLIST.0.remove(ifp) };
    net_unlock();

    // Remove the interface from the interface index map.
    if_idxmap_remove(ifp);

    // Make sure softnet threads have finished with it
    net_tq_barriers("ifrmnet");

    // Sleep until the last reference is released.
    refcnt_finalize(&ifp.if_refcnt, "ifrm");
}

/// `if_deactivate`: call detach hooks from head to tail. To make sure detach hooks are
/// executed in the reverse order they were added, all the hooks have to be added to the head!
pub fn if_deactivate(ifp: &Ifnet) {
    net_lock();
    if_hooks_run(&ifp.if_detachhooks);
    net_unlock();
}

/// `if_detachhook_add`.
///
/// # Safety
///
/// `t` is on no task list and stays valid until `if_detachhook_del` removes it (or the
/// interface is gone).
pub unsafe fn if_detachhook_add(ifp: &Ifnet, t: &Task) {
    mtx_enter(&IF_HOOKS_MTX);
    // SAFETY: the caller's contract.
    unsafe { ifp.if_detachhooks.insert_head(t) };
    mtx_leave(&IF_HOOKS_MTX);
}

/// `if_detachhook_del`.
///
/// # Safety
///
/// `t` was added with `if_detachhook_add` on this interface.
pub unsafe fn if_detachhook_del(ifp: &Ifnet, t: &Task) {
    mtx_enter(&IF_HOOKS_MTX);
    // SAFETY: the caller's contract.
    unsafe { ifp.if_detachhooks.remove(t) };
    mtx_leave(&IF_HOOKS_MTX);
}

/// `if_detach`: detach an interface from everything in the kernel. Also deallocate private
/// resources.
pub fn if_detach(ifp: &'static Ifnet) {
    // Undo pseudo-driver changes.
    if_deactivate(ifp);

    // Other CPUs must not have a reference before we start destroying.
    if_remove(ifp);

    ifp.if_qstart.set(Some(if_detached_qstart));

    // Wait until the start routines finished.
    for i in 0..ifp.if_nifqs.get() {
        ifq_barrier(ifp.ifq(i));
        ifq_clr_oactive(ifp.ifq(i));
    }

    bpfdetach(ifp);

    net_lock();
    let s = splnet();
    ifp.if_ioctl.set(Some(if_detached_ioctl));
    ifp.if_watchdog.set(None);

    // Remove the watchdog timeout & task
    let _ = timeout_del(&ifp.if_slowtimo);
    if let Some(tq) = net_tq(ifp.if_index.get()) {
        let _ = task_del(tq, &ifp.if_watchdogtask);

        // Remove the link state task
        let _ = task_del(tq, &ifp.if_linkstatetask);
    }

    rti_delete(ifp);
    // NETHER > 0 && NFSCLIENT: revarp_ifidx is cleared; NFSCLIENT is not configured.
    // MROUTING: vif_delete(ifp); not configured.
    in_ifdetach(ifp);
    #[cfg(feature = "inet6")]
    in6_ifdetach(ifp);
    pfi_detach_ifnet(ifp);

    while let Some(ifg) = ifp.if_groups.first() {
        // A copy: if_delgroup may free the group that holds the name.
        let name = ifg.ifgl_group.ifg_group;
        let _ = if_delgroup(ifp, &name);
    }

    if_free_sadl(ifp);

    // We should not have any address left at this point.
    if !ifp.if_addrlist.is_empty() {
        #[cfg(feature = "diagnostic")]
        printf(format_args!(
            "{}: address list non empty\n",
            Str(&ifp.if_xname.get())
        ));
        while let Some(ifa) = ifp.if_addrlist.first() {
            ifa_del(ifp, ifa);
            ifa.ifa_ifp.set(None);
            ifafree(ifa);
        }
    }
    splx(s);
    net_unlock();

    kassert!(ifp.if_addrhooks.is_empty());
    kassert!(ifp.if_linkstatehooks.is_empty());
    kassert!(ifp.if_detachhooks.is_empty());

    #[cfg(feature = "inet6")]
    nd6_ifdetach(ifp);

    // Announce that the interface is gone.
    rtm_ifannounce(ifp, IFAN_DEPARTURE);

    if ifp.if_counters.get().is_some() {
        if_counters_free(ifp);
    }

    for i in 0..ifp.if_nifqs.get() {
        ifq_destroy(ifp.ifq(i));
    }
    if !ptr::eq(ifp.if_ifqs.get(), ifp.if_snd.ifq_ifqs.as_ptr()) {
        for i in 1..ifp.if_nifqs.get() {
            free(
                NonNull::from(ifp.ifq(i)).cast(),
                M_DEVBUF,
                size_of::<Ifqueue>(),
            );
        }
        if let Some(map) = NonNull::new(ifp.if_ifqs.get().cast_mut()) {
            free(
                map.cast(),
                M_DEVBUF,
                size_of::<*const Ifqueue>() * ifp.if_nifqs.get() as usize,
            );
        }
    }

    for i in 0..ifp.if_niqs.get() {
        ifiq_destroy(ifp.ifiq(i));
    }
    if !ptr::eq(ifp.if_iqs.get(), ifp.if_rcv.ifiq_ifiqs.as_ptr()) {
        for i in 1..ifp.if_niqs.get() {
            free(
                NonNull::from(ifp.ifiq(i)).cast(),
                M_DEVBUF,
                size_of::<Ifiqueue>(),
            );
        }
        if let Some(map) = NonNull::new(ifp.if_iqs.get().cast_mut()) {
            free(
                map.cast(),
                M_DEVBUF,
                size_of::<*const Ifiqueue>() * ifp.if_niqs.get() as usize,
            );
        }
    }
}

/// `if_isconnected`: returns true if `ifp0` is connected to the interface with index
/// `ifidx`.
pub fn if_isconnected(ifp0: &Ifnet, ifidx: u32) -> bool {
    let Some(ifp) = if_get(ifidx) else {
        return false;
    };

    let connected = ifp0.if_index.get() == ifp.if_index.get();

    // NBRIDGE > 0: two ports of one bridge (if_bridgeidx) are connected; not configured.
    // NCARP > 0: a carp(4) interface and its carpdev are connected; not configured.

    if_put(ifp);
    connected
}

/// `if_clone_create`: create a clone network interface (`ifconfig lo1 create`).
pub fn if_clone_create(name: &[u8], rdomain: i32) -> Result<(), Errno> {
    let Some((ifc, unit)) = if_clone_lookup(name) else {
        return Err(Errno::EINVAL);
    };

    rw_enter_write(&IF_CLONERS_LOCK);

    let mut ifp: Option<&'static Ifnet>;
    let ret = 'unlock: {
        ifp = if_unit(name);
        if ifp.is_some() {
            break 'unlock Err(Errno::EEXIST);
        }

        let ret = (ifc.ifc_create)(ifc, unit);

        if ret.is_err() {
            break 'unlock ret;
        }
        ifp = if_unit(name);
        let Some(ifp) = ifp else {
            break 'unlock ret;
        };

        net_lock();
        let _ = if_addgroup(ifp, ifc.ifc_name);
        if rdomain != 0 {
            let _ = if_setrdomain(ifp, rdomain);
        }
        net_unlock();
        ret
    };
    rw_exit_write(&IF_CLONERS_LOCK);
    if_put(ifp);

    ret
}

/// `if_clone_destroy`: destroy a clone network interface.
pub fn if_clone_destroy(name: &[u8]) -> Result<(), Errno> {
    let Some((ifc, _)) = if_clone_lookup(name) else {
        return Err(Errno::EINVAL);
    };

    let Some(destroy) = ifc.ifc_destroy else {
        return Err(Errno::EOPNOTSUPP);
    };

    // KERNEL_ASSERT_LOCKED(): one CPU.
    rw_enter_write(&IF_CLONERS_LOCK);

    let Some(ifp) = IFNETLIST
        .0
        .iter()
        .find(|ifp| name_eq(&ifp.if_xname.get(), name))
    else {
        rw_exit_write(&IF_CLONERS_LOCK);
        return Err(Errno::ENXIO);
    };

    net_lock();
    if ifp.if_flags.get() & IFF_UP != 0 {
        let s = splnet();
        if_down(ifp);
        splx(s);
    }
    net_unlock();
    let ret = destroy(ifp);

    rw_exit_write(&IF_CLONERS_LOCK);

    ret
}

/// `if_clone_lookup`: look up a network interface cloner; the cloner and the unit number.
pub fn if_clone_lookup(name: &[u8]) -> Option<(&'static IfClone, i32)> {
    let name = cstr(name);

    // separate interface name from unit
    let cp = name
        .iter()
        .take(IFNAMSIZ)
        .position(u8::is_ascii_digit)
        .unwrap_or(name.len().min(IFNAMSIZ));

    if cp == 0 || cp == IFNAMSIZ || cp == name.len() {
        return None; // No name or unit number
    }

    if cp < IFNAMSIZ - 1 && name[cp] == b'0' && cp + 1 < name.len() {
        return None; // unit number 0 padded
    }

    let ifc = IF_CLONERS
        .0
        .iter()
        .find(|ifc| ifc.ifc_name.len() == cp && ifc.ifc_name == &name[..cp])?;

    let mut unit: i32 = 0;
    for &c in name[cp..].iter().take(IFNAMSIZ - cp) {
        let d = i32::from(c) - i32::from(b'0');
        if !c.is_ascii_digit() || unit > (i32::MAX - d) / 10 {
            // Bogus unit number.
            return None;
        }
        unit = unit * 10 + d;
    }

    Some((ifc, unit))
}

/// `if_clone_attach`: register a network interface cloner. We are called at kernel boot by
/// `main()`, when pseudo devices are being attached. The `main()` is the only guy which may
/// alter the `if_cloners`. While system is running and `main()` is done with initialization,
/// the `if_cloners` becomes immutable.
///
/// # Safety
///
/// `ifc` is registered once.
pub unsafe fn if_clone_attach(ifc: &'static IfClone) {
    kassert!(!PDEVINIT_DONE.load(Ordering::Relaxed));
    // SAFETY: the caller's contract: the cloner is on no list; it is a `'static`.
    unsafe { IF_CLONERS.0.insert_head(ifc) };
    IF_CLONERS_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// `if_clone_list`: provide list of interface cloners to userspace.
pub fn if_clone_list(ifcr: &mut IfClonereq) -> Result<(), Errno> {
    let count_all = IF_CLONERS_COUNT.load(Ordering::Relaxed);

    let mut dst = ifcr.ifcr_buffer as usize;
    if dst == 0 {
        // Just asking how many there are.
        ifcr.ifcr_total = count_all;
        return Ok(());
    }

    if ifcr.ifcr_count < 0 {
        return Err(Errno::EINVAL);
    }

    ifcr.ifcr_total = count_all;
    let mut count = min(count_all, ifcr.ifcr_count);

    let mut error = Ok(());
    for ifc in IF_CLONERS.0.iter() {
        if count == 0 {
            break;
        }
        let mut outbuf = [0u8; IFNAMSIZ];
        let _ = strlcpy(&mut outbuf, ifc.ifc_name);
        error = copyout(&outbuf, dst);
        if error.is_err() {
            break;
        }
        count -= 1;
        dst += IFNAMSIZ;
    }

    error
}

/// `if_congestion`: set queue congestion marker.
pub fn if_congestion() {
    IFQ_CONGESTION.store(TICKS.load(Ordering::Relaxed), Ordering::Relaxed);
}

/// `if_congested`: whether a queue was congested in the last hundredth of a second.
pub fn if_congested() -> bool {
    let ticks = TICKS.load(Ordering::Relaxed);
    let diff = ticks.wrapping_sub(IFQ_CONGESTION.load(Ordering::Relaxed));
    if diff < 0 {
        IFQ_CONGESTION.store(ticks.wrapping_sub(HZ), Ordering::Relaxed);
        return false;
    }

    diff <= HZ / 100
}

/// `equal(a1, a2)`: the two socket addresses are the same `a1->sa_len` bytes.
///
/// # Safety
///
/// Both point at readable socket addresses at least `a1->sa_len` bytes long.
unsafe fn equal(a1: *const Sockaddr, a2: *const Sockaddr) -> bool {
    // SAFETY: the caller's contract.
    unsafe {
        let len = usize::from((*a1).sa_len);
        slice::from_raw_parts(a1.cast::<u8>(), len) == slice::from_raw_parts(a2.cast::<u8>(), len)
    }
}

/// An interface address with the lifetime the address list gives it.
fn ifa_static(ifa: &Ifaddr) -> &'static Ifaddr {
    // SAFETY: an address on an interface's list lives until `ifa_del` and the last `ifafree`,
    // as the C's pointers do (`docs/C_TO_RUST.md`, reference-counted pool objects).
    unsafe { &*ptr::from_ref(ifa) }
}

/// `ifa_ifwithaddr`: locate an interface based on a complete address.
///
/// # Safety
///
/// `addr` points at a readable socket address of `sa_len` bytes; the addresses of the
/// interfaces are valid (`ifa_add`'s contract).
pub unsafe fn ifa_ifwithaddr(addr: *const Sockaddr, rtableid: u32) -> Option<&'static Ifaddr> {
    net_assert_locked("ifa_ifwithaddr");

    let rdomain = rtable_l2(rtableid);
    for ifp in IFNETLIST.0.iter() {
        if ifp.if_rdomain.get() != rdomain {
            continue;
        }

        for ifa in ifp.if_addrlist.iter() {
            let ifa_addr = ifa.ifa_addr.get();
            // SAFETY: the caller's contract covers both addresses.
            unsafe {
                if (*ifa_addr).sa_family != (*addr).sa_family {
                    continue;
                }

                if equal(addr, ifa_addr) {
                    return Some(ifa_static(ifa));
                }
            }
        }
    }
    None
}

/// `ifa_ifwithdstaddr`: locate the point to point interface with a given destination
/// address.
///
/// # Safety
///
/// As for [`ifa_ifwithaddr`].
pub unsafe fn ifa_ifwithdstaddr(addr: *const Sockaddr, rdomain: u32) -> Option<&'static Ifaddr> {
    net_assert_locked("ifa_ifwithdstaddr");

    let rdomain = rtable_l2(rdomain);
    for ifp in IFNETLIST.0.iter() {
        if ifp.if_rdomain.get() != rdomain {
            continue;
        }
        if ifp.if_flags.get() & IFF_POINTOPOINT != 0 {
            for ifa in ifp.if_addrlist.iter() {
                let dstaddr = ifa.ifa_dstaddr.get();
                // SAFETY: the caller's contract covers the addresses.
                unsafe {
                    if (*ifa.ifa_addr.get()).sa_family != (*addr).sa_family || dstaddr.is_null() {
                        continue;
                    }
                    if equal(addr, dstaddr) {
                        return Some(ifa_static(ifa));
                    }
                }
            }
        }
    }
    None
}

/// `ifaof_ifpforaddr`: find an interface address specific to an interface best matching a
/// given address.
///
/// # Safety
///
/// As for [`ifa_ifwithaddr`]; a netmask is a socket address of `sa_len` bytes.
pub unsafe fn ifaof_ifpforaddr(addr: *const Sockaddr, ifp: &Ifnet) -> Option<&'static Ifaddr> {
    let mut ifa_maybe = None;
    // SAFETY: `addr` is readable.
    let af = unsafe { (*addr).sa_family };

    if af >= AF_MAX {
        return None;
    }
    for ifa in ifp.if_addrlist.iter() {
        let ifa_addr = ifa.ifa_addr.get();
        // SAFETY: an address of the list is readable (the caller's contract).
        if unsafe { (*ifa_addr).sa_family } != af {
            continue;
        }
        if ifa_maybe.is_none() {
            ifa_maybe = Some(ifa_static(ifa));
        }
        let netmask = ifa.ifa_netmask.get();
        if netmask.is_null() || ifp.if_flags.get() & IFF_POINTOPOINT != 0 {
            let dstaddr = ifa.ifa_dstaddr.get();
            // SAFETY: the caller's contract covers the addresses.
            if unsafe { equal(addr, ifa_addr) || (!dstaddr.is_null() && equal(addr, dstaddr)) } {
                return Some(ifa_static(ifa));
            }
            continue;
        }
        // Compare the bytes after sa_len/sa_family (sa_data) under the netmask, up to the
        // netmask's length.
        let data = offset_of!(Sockaddr, sa_data);
        // SAFETY: the netmask is `sa_len` readable bytes; `addr` and the address are at least
        // as long as the netmask's significant part (sockaddrs of one family).
        let matches = unsafe {
            let masklen = usize::from((*netmask).sa_len);
            let cp = addr.cast::<u8>();
            let cp2 = ifa_addr.cast::<u8>();
            let cp3 = netmask.cast::<u8>();
            (data..masklen).all(|i| (*cp.add(i) ^ *cp2.add(i)) & *cp3.add(i) == 0)
        };
        if matches {
            return Some(ifa_static(ifa));
        }
    }
    ifa_maybe
}

/// `if_rtrequest_dummy`: the default `if_rtrequest`.
pub fn if_rtrequest_dummy(_ifp: &'static Ifnet, _req: i32, _rt: Option<&'static Rtentry>) {}

/// `p2p_rtrequest`: default action when installing a local route on a point-to-point
/// interface.
pub fn p2p_rtrequest(ifp: &'static Ifnet, req: i32, rt: Option<&'static Rtentry>) {
    let Some(rt) = rt else {
        return;
    };
    if i32::from(RTM_ADD) != req {
        // RTM_DELETE, RTM_RESOLVE, default: nothing.
        return;
    }
    if rt.rt_flags.get() & RTF_LOCAL == 0 {
        return;
    }

    let key = rt_key(rt);
    let Some(ifa) = ifp.if_addrlist.iter().find(|ifa| {
        // SAFETY: a route's key and an interface's addresses are valid sockaddrs.
        unsafe {
            let len = usize::from((*key).sa_len);
            slice::from_raw_parts(key.cast::<u8>(), len)
                == slice::from_raw_parts(ifa.ifa_addr.get().cast::<u8>(), len)
        }
    }) else {
        return;
    };

    kassert!(rt.rt_ifa.get().is_some_and(|rifa| ptr::eq(rifa, ifa)));

    let lo0ifp = if_get(rtable_loindex(ifp.if_rdomain.get()));
    kassert!(lo0ifp.is_some());
    let family = {
        // SAFETY: as above.
        unsafe { (*ifa.ifa_addr.get()).sa_family }
    };
    let lo0ifa = lo0ifp.and_then(|lo0ifp| {
        lo0ifp
            .if_addrlist
            .iter()
            // SAFETY: as above.
            .find(|lo0ifa| unsafe { (*lo0ifa.ifa_addr.get()).sa_family } == family)
            .map(ptr::from_ref)
    });
    if_put(lo0ifp);

    if lo0ifa.is_none() {
        return;
    }

    rt.rt_flags.set(rt.rt_flags.get() & !RTF_LLINFO);
}

/// `p2p_bpf_mtap`: a point-to-point interface's tap, by the packet's address family.
pub fn p2p_bpf_mtap(if_bpf: *mut u8, m: &Mbuf, dir: u32) -> bool {
    bpf_mtap_af(if_bpf, u32::from(m.m_pkthdr().ph_family.get()), m, dir)
}

/// `p2p_input`: a point-to-point interface's input, by the packet's address family.
pub fn p2p_input(ifp: &'static Ifnet, m: &'static Mbuf, ns: Option<&Netstack>) {
    match m.m_pkthdr().ph_family.get() {
        AF_INET => if_input_proto(ifp, m, ipv4_input, ns),
        #[cfg(feature = "inet6")]
        AF_INET6 => if_input_proto(ifp, m, ipv6_input, ns),
        // MPLS: mpls_input; MPLS is not configured.
        _ => {
            m_freem(m);
        }
    }
}

/// `if_downall`: bring down all interfaces.
pub fn if_downall() {
    let mut ifrq = Ifreq::zeroed(); // XXX only partly built

    net_lock();
    for ifp in IFNETLIST.0.iter() {
        if ifp.if_flags.get() & IFF_UP == 0 {
            continue;
        }
        if_down(ifp);
        ifrq.set_ifr_flags(ifp.if_flags.get() as i16);
        // SAFETY: `ifrq` is a `struct ifreq`, what `SIOCSIFFLAGS` takes.
        let _ = unsafe { ifp_ioctl(ifp, SIOCSIFFLAGS, ptr::from_mut(&mut ifrq).cast()) };
    }
    net_unlock();
}

/// `if_down`: mark an interface down and notify protocols of the transition.
pub fn if_down(ifp: &Ifnet) {
    net_assert_locked("if_down");

    ifp.if_flags.set(ifp.if_flags.get() & !IFF_UP);
    ifp.if_lastchange.set(getmicrotime());
    let _ = ifq_purge(&ifp.if_snd);

    if_linkstate(ifp);
}

/// `if_up`: mark an interface up and notify protocols of the transition.
pub fn if_up(ifp: &'static Ifnet) {
    net_assert_locked("if_up");

    ifp.if_flags.set(ifp.if_flags.get() | IFF_UP);
    ifp.if_lastchange.set(getmicrotime());

    // Userland expects the kernel to set ::1 on default lo(4).
    #[cfg(feature = "inet6")]
    if ifp.if_index.get() == rtable_loindex(ifp.if_rdomain.get()) {
        // The C ignores the result.
        let _ = in6_ifattach(ifp);
    }

    if_linkstate(ifp);
}

/// `if_linkstate_task`: notify userland, the routing table and hooks owner of a link-state
/// transition.
fn if_linkstate_task(xifidx: *mut c_void) {
    let ifidx = xifidx.addr() as u32;

    net_lock();
    // KERNEL_LOCK(): one CPU.

    let ifp = if_get(ifidx);
    if let Some(ifp) = ifp {
        if_linkstate(ifp);
    }
    if_put(ifp);

    // KERNEL_UNLOCK()
    net_unlock();
}

/// `if_linkstate`: tells the routing socket and the routing table, then runs the link-state
/// hooks.
pub fn if_linkstate(ifp: &Ifnet) {
    net_assert_locked("if_linkstate");

    if !panicstr() {
        rtm_ifchg(ifp);
        let _ = rt_if_track(ifp);
    }

    if_hooks_run(&ifp.if_linkstatehooks);
}

/// `if_linkstatehook_add`.
///
/// # Safety
///
/// As for [`if_detachhook_add`].
pub unsafe fn if_linkstatehook_add(ifp: &Ifnet, t: &Task) {
    mtx_enter(&IF_HOOKS_MTX);
    // SAFETY: the caller's contract.
    unsafe { ifp.if_linkstatehooks.insert_head(t) };
    mtx_leave(&IF_HOOKS_MTX);
}

/// `if_linkstatehook_del`.
///
/// # Safety
///
/// `t` was added with `if_linkstatehook_add` on this interface.
pub unsafe fn if_linkstatehook_del(ifp: &Ifnet, t: &Task) {
    mtx_enter(&IF_HOOKS_MTX);
    // SAFETY: the caller's contract.
    unsafe { ifp.if_linkstatehooks.remove(t) };
    mtx_leave(&IF_HOOKS_MTX);
}

/// `if_link_state_change`: schedule a link state change task (a driver saw its link change).
pub fn if_link_state_change(ifp: &'static Ifnet) {
    if let Some(tq) = net_tq(ifp.if_index.get()) {
        let _ = task_add(tq, &ifp.if_linkstatetask);
    }
}

/// `if_slowtimo`: handle interface watchdog timer routine. Called from softclock, we
/// decrement timer (if set) and call the appropriate interface routine on expiration.
fn if_slowtimo(arg: *mut c_void) {
    // SAFETY: `if_attachsetup` armed the timeout with the interface, which lives until
    // `if_detach` deletes the timeout.
    let ifp: &'static Ifnet = unsafe { &*arg.cast::<Ifnet>() };
    let s = splnet();

    if ifp.if_watchdog.get().is_some() {
        let timer = ifp.if_timer.get();
        if timer > 0 {
            ifp.if_timer.set(timer - 1);
            if timer - 1 == 0
                && let Some(tq) = net_tq(ifp.if_index.get())
            {
                let _ = task_add(tq, &ifp.if_watchdogtask);
            }
        }
        let _ = timeout_add_sec(&ifp.if_slowtimo, IFNET_SLOWTIMO);
    }
    splx(s);
}

/// `if_watchdog_task`: runs the driver's watchdog.
fn if_watchdog_task(xifidx: *mut c_void) {
    let ifidx = xifidx.addr() as u32;

    let Some(ifp) = if_get(ifidx) else {
        return;
    };

    // KERNEL_LOCK(): one CPU.
    let s = splnet();
    if let Some(watchdog) = ifp.if_watchdog.get() {
        watchdog(ifp);
    }
    splx(s);
    // KERNEL_UNLOCK()

    if_put(ifp);
}

/// `if_unit`: map interface name to interface structure pointer, with a reference.
pub fn if_unit(name: &[u8]) -> Option<&'static Ifnet> {
    // KERNEL_ASSERT_LOCKED(): one CPU.

    let ifp = IFNETLIST
        .0
        .iter()
        .find(|ifp| name_eq(&ifp.if_xname.get(), name))?;
    Some(if_ref(ifp))
}

/// `if_get`: map interface index to interface structure pointer, with a reference that
/// `if_put` releases.
pub fn if_get(index: u32) -> Option<&'static Ifnet> {
    if index == 0 {
        return None;
    }

    // smr_read_enter(): see the module's deviations.
    let ifp = if_idxmap_get(index)?;
    kassert!(ifp.if_index.get() == index);
    // smr_read_leave()

    Some(if_ref(ifp))
}

/// `if_get_smr`: the interface at `index` inside an SMR read section, without a reference.
pub fn if_get_smr(index: u32) -> Option<&'static Ifnet> {
    // SMR_ASSERT_CRITICAL(): no SMR (see the module's deviations).
    if_idxmap_get(index)
}

/// `if_ref`: takes a reference to the interface.
pub fn if_ref(ifp: &Ifnet) -> &Ifnet {
    refcnt_take(&ifp.if_refcnt);

    ifp
}

/// `if_put`: releases a reference from `if_get`, `if_unit` or `if_ref`; NULL is fine.
pub fn if_put<'a>(ifp: impl Into<Option<&'a Ifnet>>) {
    let Some(ifp) = ifp.into() else {
        return;
    };

    refcnt_rele_wake(&ifp.if_refcnt);
}

/// `if_setlladdr`: sets the Ethernet address of an Ethernet-like interface.
pub fn if_setlladdr(ifp: &Ifnet, lladdr: &[u8; ETHER_ADDR_LEN]) -> Result<(), Errno> {
    let sdl = ifp.if_sadl.get();
    if sdl.is_null() {
        return Err(Errno::EINVAL);
    }

    arpcom_of(ifp).ac_enaddr.set(*lladdr);
    // SAFETY: `if_sadl` was allocated by `if_alloc_sadl` with room for `if_addrlen` (the
    // Ethernet address's length) after the name.
    unsafe { ptr::copy_nonoverlapping(lladdr.as_ptr(), lladdr_of(sdl), ETHER_ADDR_LEN) };

    Ok(())
}

/// `LLADDR(sdl)` of the interface's own link address.
///
/// # Safety
///
/// `sdl` is an interface's `if_sadl`.
unsafe fn lladdr_of(sdl: *mut SockaddrDl) -> *mut u8 {
    // SAFETY: the caller's contract.
    unsafe { lladdr(sdl) }
}

/// `if_createrdomain`: creates routing domain `rdomain` with its loopback `lo<rdomain>`.
pub fn if_createrdomain(rdomain: i32, ifp: &Ifnet) -> Result<(), Errno> {
    rtable_add(rdomain as u32)?;
    if !rtable_empty(rdomain as u32) {
        return Err(Errno::EEXIST);
    }

    // Create rdomain including its loopback if with unit == rdomain
    let mut loifname = [0u8; IFNAMSIZ];
    let _ = snprintf(&mut loifname, format_args!("lo{}", rdomain as u32));
    let error = if_clone_create(&loifname, 0);
    let Some(loifp) = if_unit(&loifname) else {
        return Err(Errno::ENXIO);
    };
    if let Err(e) = error
        && (!ptr::eq(ifp, loifp) || e != Errno::EEXIST)
    {
        if_put(loifp);
        return Err(e);
    }

    rtable_l2set(rdomain as u32, rdomain as u32, loifp.if_index.get());
    loifp.if_rdomain.set(rdomain as u32);
    if_put(loifp);

    Ok(())
}

/// `if_setrdomain`: moves the interface to routing domain `rdomain`.
pub fn if_setrdomain(ifp: &'static Ifnet, rdomain: i32) -> Result<(), Errno> {
    let mut up = false;

    if rdomain < 0 || rdomain as u32 > RT_TABLEID_MAX {
        return Err(Errno::EINVAL);
    }
    let rdomain = rdomain as u32;

    if rdomain != ifp.if_rdomain.get()
        && ifp.if_flags.get() & IFF_LOOPBACK != 0
        && ifp.if_index.get() == rtable_loindex(ifp.if_rdomain.get())
    {
        return Err(Errno::EPERM);
    }

    if !rtable_exists(rdomain) {
        return Err(Errno::ESRCH);
    }

    // make sure that the routing table is a real rdomain
    if rdomain != rtable_l2(rdomain) {
        return Err(Errno::EINVAL);
    }

    if rdomain != ifp.if_rdomain.get() {
        let s = splnet();
        // We are tearing down the world. Take down the IF so:
        // 1. everything that cares gets a message
        // 2. the automagic IPv6 bits are recreated
        if ifp.if_flags.get() & IFF_UP != 0 {
            up = true;
            if_down(ifp);
        }
        rti_delete(ifp);
        // MROUTING: vif_delete(ifp); not configured.
        in_ifdetach(ifp);
        #[cfg(feature = "inet6")]
        in6_ifdetach(ifp);
        splx(s);
    }

    // Let devices like enc(4) or mpe(4) know about the change
    let mut ifr = Ifreq::zeroed();
    ifr.set_ifr_rdomainid(rdomain as i32);
    // SAFETY: `ifr` is a `struct ifreq`, what `SIOCSIFRDOMAIN` takes.
    match unsafe { ifp_ioctl(ifp, SIOCSIFRDOMAIN, ptr::from_mut(&mut ifr).cast()) } {
        Err(Errno::ENOTTY) => {}
        error => return error,
    }

    // Add interface to the specified rdomain
    ifp.if_rdomain.set(rdomain);

    // If we took down the IF, bring it back
    if up {
        let s = splnet();
        if_up(ifp);
        splx(s);
    }

    Ok(())
}

/// `pru_control(so, cmd, data, ifp)` (`<sys/protosw.h>`) of the socket of the request: the
/// protocol's `ioctl`. A request of the kernel itself (no socket) goes to `in_ioctl` as a
/// privileged one (see the module's deviations).
///
/// # Safety
///
/// As for [`ifioctl`].
unsafe fn pru_control(
    so: *const c_void,
    cmd: u64,
    data: *mut u8,
    ifp: &'static Ifnet,
) -> Result<(), Errno> {
    if so.is_null() {
        // SAFETY: the caller's contract.
        return unsafe { in_ioctl(cmd, data, Some(ifp), true) };
    }
    // SAFETY: a non-NULL `so` is the live socket of the request, which the caller's file
    // reference keeps for the call (the `fp_socket` idiom).
    let so: &'static Socket = unsafe { &*so.cast::<Socket>() };
    // SAFETY: `data` is the kernel copy of the request, as long as `cmd` encodes (the
    // caller's contract), exclusively ours for the call.
    let data = unsafe { slice::from_raw_parts_mut(data, iocparm_len(cmd) as usize) };
    protosw::pru_control(so, cmd, data, Some(ifp))
}

/// `ifioctl`: interface ioctls (`SIOC*` on a socket).
///
/// # Safety
///
/// `data` points at the kernel copy of the request, aligned for and as long as the structure
/// `cmd` encodes; `so` is the live `struct socket` of the request, or NULL.
pub unsafe fn ifioctl(so: *const c_void, cmd: u64, data: *mut u8, p: &Proc) -> Result<(), Errno> {
    // SAFETY: every interface command's argument starts with the interface name (`ifreq`,
    // `ifgroupreq`, `if_afreq`), per the caller's contract.
    let ifr = unsafe { &mut *data.cast::<Ifreq>() };

    match cmd {
        SIOCIFCREATE => {
            suser(p)?;
            // KERNEL_LOCK(): one CPU.
            return if_clone_create(&ifr.ifr_name, 0);
        }
        SIOCIFDESTROY => {
            suser(p)?;
            return if_clone_destroy(&ifr.ifr_name);
        }
        SIOCSIFGATTR => {
            suser(p)?;
            net_lock();
            // SAFETY: the caller's contract: an `ifgroupreq`.
            let error = unsafe { if_setgroupattribs(data) };
            net_unlock();
            return error;
        }
        SIOCGIFCONF | SIOCIFGCLONERS | SIOCGIFGMEMB | SIOCGIFGATTR | SIOCGIFGLIST
        | SIOCGIFFLAGS | SIOCGIFXFLAGS | SIOCGIFMETRIC | SIOCGIFMTU | SIOCGIFHARDMTU
        | SIOCGIFDATA | SIOCGIFDESCR | SIOCGIFRTLABEL | SIOCGIFPRIORITY | SIOCGIFRDOMAIN
        | SIOCGIFGROUP | SIOCGIFLLPRIO => {
            // SAFETY: the caller's contract.
            return unsafe { ifioctl_get(cmd, data) };
        }
        _ => {}
    }

    // KERNEL_LOCK(): one CPU.

    let Some(ifp) = if_unit(&ifr.ifr_name) else {
        return Err(Errno::ENXIO);
    };
    let oif_flags = ifp.if_flags.get();
    let oif_xflags = ifp.if_xflags.get();

    // The C's `goto forceup` from SIOCSIFXFLAGS into SIOCSIFFLAGS.
    let mut forceup = false;
    let mut error: Result<(), Errno> = Ok(());

    match cmd {
        SIOCIFAFATTACH | SIOCIFAFDETACH => 'out: {
            if let Err(e) = suser(p) {
                error = Err(e);
                break 'out;
            }
            // SAFETY: the caller's contract: an `if_afreq`.
            let ifar = unsafe { &*data.cast::<IfAfreq>() };
            net_lock();
            match ifar.ifar_af {
                AF_INET => {
                    // attach is a noop for AF_INET
                    if cmd == SIOCIFAFDETACH {
                        in_ifdetach(ifp);
                    }
                }
                #[cfg(feature = "inet6")]
                AF_INET6 => {
                    if cmd == SIOCIFAFATTACH {
                        error = in6_ifattach(ifp);
                    } else {
                        in6_ifdetach(ifp);
                    }
                }
                _ => error = Err(Errno::EAFNOSUPPORT),
            }
            net_unlock();
        }

        SIOCSIFXFLAGS => 'out: {
            if let Err(e) = suser(p) {
                error = Err(e);
                break 'out;
            }

            net_lock();
            #[cfg(feature = "inet6")]
            {
                let rflags = i32::from(ifr.ifr_flags());
                let xflags = ifp.if_xflags.get();
                if rflags & (IFXF_AUTOCONF6 | IFXF_AUTOCONF6TEMP) != 0
                    && xflags & (IFXF_AUTOCONF6 | IFXF_AUTOCONF6TEMP) == 0
                    && let Err(e) = in6_ifattach(ifp)
                {
                    error = Err(e);
                    net_unlock();
                    break 'out;
                }

                if rflags & IFXF_INET6_NOSOII != 0 && ifp.if_xflags.get() & IFXF_INET6_NOSOII == 0 {
                    ifp.if_xflags.set(ifp.if_xflags.get() | IFXF_INET6_NOSOII);
                }

                if rflags & IFXF_INET6_NOSOII == 0 && ifp.if_xflags.get() & IFXF_INET6_NOSOII != 0 {
                    ifp.if_xflags.set(ifp.if_xflags.get() & !IFXF_INET6_NOSOII);
                }
            }

            // MPLS: IFXF_MPLS swaps if_output with mpls_output; MPLS is not configured.

            // !SMALL_KERNEL
            let rflags = i32::from(ifr.ifr_flags());
            if ifp.if_capabilities.get() & IFCAP_WOL != 0 {
                if rflags & IFXF_WOL != 0 && ifp.if_xflags.get() & IFXF_WOL == 0 {
                    let s = splnet();
                    ifp.if_xflags.set(ifp.if_xflags.get() | IFXF_WOL);
                    error = match ifp.if_wol.get() {
                        Some(wol) => wol(ifp, true),
                        None => Err(Errno::ENOTSUP),
                    };
                    splx(s);
                }
                if ifp.if_xflags.get() & IFXF_WOL != 0 && rflags & IFXF_WOL == 0 {
                    let s = splnet();
                    ifp.if_xflags.set(ifp.if_xflags.get() & !IFXF_WOL);
                    error = match ifp.if_wol.get() {
                        Some(wol) => wol(ifp, false),
                        None => Err(Errno::ENOTSUP),
                    };
                    splx(s);
                }
            } else if rflags & IFXF_WOL != 0 {
                ifr.set_ifr_flags((rflags & !IFXF_WOL) as i16);
                error = Err(Errno::ENOTSUP);
            }
            let rflags = i32::from(ifr.ifr_flags());
            if (rflags & IFXF_LRO != 0) != (ifp.if_xflags.get() & IFXF_LRO != 0) {
                error = ifsetlro(ifp, rflags & IFXF_LRO != 0);
            }

            if error.is_ok() {
                ifp.if_xflags
                    .set((ifp.if_xflags.get() & IFXF_CANTCHANGE) | (rflags & !IFXF_CANTCHANGE));
            }

            let xflags = ifp.if_xflags.get();
            if ifp.if_flags.get() & IFF_UP == 0
                && ((oif_xflags & IFXF_AUTOCONF4 == 0 && xflags & IFXF_AUTOCONF4 != 0)
                    || (oif_xflags & IFXF_AUTOCONF6 == 0 && xflags & IFXF_AUTOCONF6 != 0)
                    || (oif_xflags & IFXF_AUTOCONF6TEMP == 0 && xflags & IFXF_AUTOCONF6TEMP != 0))
            {
                ifr.set_ifr_flags((ifp.if_flags.get() | IFF_UP) as i16);
                forceup = true;
                break 'out;
            }

            net_unlock();
        }

        SIOCSIFFLAGS => 'out: {
            if let Err(e) = suser(p) {
                error = Err(e);
                break 'out;
            }

            net_lock();
            forceup = true;
        }

        SIOCSIFMETRIC => 'out: {
            if let Err(e) = suser(p) {
                error = Err(e);
                break 'out;
            }
            net_lock();
            ifp.if_metric.set(ifr.ifr_metric() as u32);
            net_unlock();
        }

        SIOCSIFMTU => 'out: {
            if let Err(e) = suser(p) {
                error = Err(e);
                break 'out;
            }
            net_lock();
            // SAFETY: the caller's contract is the hook's.
            error = unsafe { ifp_ioctl(ifp, cmd, data) };
            net_unlock();
            if error.is_ok() {
                rtm_ifchg(ifp);
            }
        }

        SIOCSIFDESCR => 'out: {
            if let Err(e) = suser(p) {
                error = Err(e);
                break 'out;
            }
            let mut ifdescrbuf = [0u8; IFDESCRSIZE];
            error = copyinstr(ifr.ifr_data() as usize, &mut ifdescrbuf).map(|_| ());
            if error.is_ok() {
                let mut descr = [0u8; IFDESCRSIZE];
                let _ = strlcpy(&mut descr, &ifdescrbuf);
                ifp.if_description.set(descr);
            }
        }

        SIOCSIFRTLABEL => 'out: {
            if let Err(e) = suser(p) {
                error = Err(e);
                break 'out;
            }
            let mut ifrtlabelbuf = [0u8; RTLABEL_LEN];
            error = copyinstr(ifr.ifr_data() as usize, &mut ifrtlabelbuf).map(|_| ());
            if error.is_ok() {
                rtlabel_unref(ifp.if_rtlabelid.get());
                ifp.if_rtlabelid.set(rtlabel_name2id(&ifrtlabelbuf));
            }
        }

        SIOCSIFPRIORITY => 'out: {
            if let Err(e) = suser(p) {
                error = Err(e);
                break 'out;
            }
            let metric = ifr.ifr_metric();
            if !(0..=15).contains(&metric) {
                error = Err(Errno::EINVAL);
                break 'out;
            }
            ifp.if_priority.set(metric as u8);
        }

        SIOCSIFRDOMAIN => 'out: {
            if let Err(e) = suser(p) {
                error = Err(e);
                break 'out;
            }
            error = if_createrdomain(ifr.ifr_rdomainid(), ifp);
            if matches!(error, Ok(()) | Err(Errno::EEXIST)) {
                net_lock();
                error = if_setrdomain(ifp, ifr.ifr_rdomainid());
                net_unlock();
            }
        }

        SIOCAIFGROUP => 'out: {
            if let Err(e) = suser(p) {
                error = Err(e);
                break 'out;
            }
            // SAFETY: the caller's contract: an `ifgroupreq`.
            let ifgr = unsafe { &*data.cast::<Ifgroupreq>() };
            net_lock();
            error = if_addgroup(ifp, ifgr.ifgr_group());
            if error.is_ok() {
                // SAFETY: the caller's contract is the hook's.
                error = unsafe { ifp_ioctl(ifp, cmd, data) };
                if error == Err(Errno::ENOTTY) {
                    error = Ok(());
                }
            }
            net_unlock();
        }

        SIOCDIFGROUP => 'out: {
            if let Err(e) = suser(p) {
                error = Err(e);
                break 'out;
            }
            // SAFETY: the caller's contract: an `ifgroupreq`.
            let ifgr = unsafe { &*data.cast::<Ifgroupreq>() };
            net_lock();
            // SAFETY: the caller's contract is the hook's.
            error = unsafe { ifp_ioctl(ifp, cmd, data) };
            if error == Err(Errno::ENOTTY) {
                error = Ok(());
            }
            if error.is_ok() {
                error = if_delgroup(ifp, ifgr.ifgr_group());
            }
            net_unlock();
        }

        SIOCSIFLLADDR => 'out: {
            if let Err(e) = suser(p) {
                error = Err(e);
                break 'out;
            }
            let sa = ifr.ifr_addr();
            let mut lladdr = [0u8; ETHER_ADDR_LEN];
            lladdr.copy_from_slice(&sa.sa_data[..ETHER_ADDR_LEN]);
            if ifp.if_sadl.get().is_null()
                || usize::from(sa.sa_len) != ETHER_ADDR_LEN
                || ether_is_multicast(&lladdr)
            {
                error = Err(Errno::EINVAL);
                break 'out;
            }
            net_lock();
            match ifp.if_type.get() {
                IFT_ETHER | IFT_CARP | IFT_XETHER | IFT_ISO88025 => {
                    // SAFETY: the caller's contract is the hook's.
                    error = unsafe { ifp_ioctl(ifp, cmd, data) };
                    if error == Err(Errno::ENOTTY) {
                        error = Ok(());
                    }
                    if error.is_ok() {
                        error = if_setlladdr(ifp, &lladdr);
                    }
                }
                _ => error = Err(Errno::ENODEV),
            }

            if error.is_ok() {
                ifnewlladdr(ifp);
            }
            net_unlock();
            if error.is_ok() {
                rtm_ifchg(ifp);
            }
        }

        SIOCSIFLLPRIO => 'out: {
            if let Err(e) = suser(p) {
                error = Err(e);
                break 'out;
            }
            let llprio = ifr.ifr_llprio();
            if llprio < IFQ_MINPRIO as i32 || llprio > IFQ_MAXPRIO as i32 {
                error = Err(Errno::EINVAL);
                break 'out;
            }
            net_lock();
            ifp.if_llprio.set(llprio as u8);
            net_unlock();
        }

        SIOCGIFSFFPAGE => 'out: {
            if let Err(e) = suser(p) {
                error = Err(e);
                break 'out;
            }

            // SAFETY: the caller's contract: an `if_sffpage`.
            error = unsafe { if_sffpage_check(data) };
            if error.is_err() {
                break 'out;
            }

            // don't take NET_LOCK because i2c reads take a long time
            // SAFETY: the caller's contract is the hook's.
            error = unsafe { ifp_ioctl(ifp, cmd, data) };
        }

        SIOCSIFMEDIA | SIOCGIFMEDIA => 'out: {
            if cmd == SIOCSIFMEDIA
                && let Err(e) = suser(p)
            {
                error = Err(e);
                break 'out;
            }
            // net lock is not needed
            // SAFETY: the caller's contract is the hook's.
            error = unsafe { ifp_ioctl(ifp, cmd, data) };
        }

        _ => 'out: {
            // These need the superuser before they go to the protocol and the driver
            // (NBRIDGE > 0 adds the SIOCBRDG* commands; bridge(4) is not configured).
            if matches!(
                cmd,
                SIOCSETKALIVE
                    | SIOCDIFPHYADDR
                    | SIOCSLIFPHYADDR
                    | SIOCSLIFPHYRTABLE
                    | SIOCSLIFPHYTTL
                    | SIOCSLIFPHYDF
                    | SIOCSLIFPHYECN
                    | SIOCADDMULTI
                    | SIOCDELMULTI
                    | SIOCSVNETID
                    | SIOCDVNETID
                    | SIOCSVNETFLOWID
                    | SIOCSTXHPRIO
                    | SIOCSRXHPRIO
                    | SIOCSIFPAIR
                    | SIOCSIFPARENT
                    | SIOCDIFPARENT
                    | SIOCSETMPWCFG
                    | SIOCSETLABEL
                    | SIOCDELLABEL
                    | SIOCSPWE3CTRLWORD
                    | SIOCSPWE3FAT
                    | SIOCSPWE3NEIGHBOR
                    | SIOCDPWE3NEIGHBOR
            ) && let Err(e) = suser(p)
            {
                error = Err(e);
                break 'out;
            }
            // SAFETY: the caller's contract.
            error = unsafe { pru_control(so, cmd, data, ifp) };
            if error != Err(Errno::EOPNOTSUPP) {
                break 'out;
            }
            error = match cmd {
                SIOCAIFADDR | SIOCDIFADDR | SIOCSIFADDR | SIOCSIFNETMASK | SIOCSIFDSTADDR
                | SIOCSIFBRDADDR => suser(p),
                #[cfg(feature = "inet6")]
                SIOCAIFADDR_IN6 | SIOCDIFADDR_IN6 => suser(p),
                _ => Ok(()),
            };
            if error.is_err() {
                break 'out;
            }
            net_lock();
            // SAFETY: the caller's contract is the hook's.
            error = unsafe { ifp_ioctl(ifp, cmd, data) };
            net_unlock();
        }
    }

    if forceup {
        // forceup: (SIOCSIFFLAGS, and SIOCSIFXFLAGS bringing an autoconf interface up)
        // The flags are an `unsigned short` in C: the assignment keeps the low sixteen bits.
        let rflags = i32::from(ifr.ifr_flags()) & 0xffff;
        ifp.if_flags
            .set((ifp.if_flags.get() & IFF_CANTCHANGE) | (rflags & !IFF_CANTCHANGE));
        // SAFETY: the caller's contract: `data` is the `ifreq`.
        error = unsafe { ifp_ioctl(ifp, SIOCSIFFLAGS, data) };
        if error.is_err() {
            ifp.if_flags.set(oif_flags);
            if cmd == SIOCSIFXFLAGS {
                ifp.if_xflags.set(oif_xflags);
            }
        } else if (oif_flags ^ ifp.if_flags.get()) & IFF_UP != 0 {
            let s = splnet();
            if ifp.if_flags.get() & IFF_UP != 0 {
                if_up(ifp);
            } else {
                if_down(ifp);
            }
            splx(s);
        }
        net_unlock();
    }

    if oif_flags != ifp.if_flags.get() || oif_xflags != ifp.if_xflags.get() {
        // if_up() and if_down() already sent an update, skip here
        if (oif_flags ^ ifp.if_flags.get()) & IFF_UP == 0 {
            rtm_ifchg(ifp);
        }
    }

    if (oif_flags ^ ifp.if_flags.get()) & IFF_UP != 0 {
        ifp.if_lastchange.set(getmicrotime());
    }

    // KERNEL_UNLOCK()

    if_put(ifp);

    error
}

/// `ifioctl_get`: the read-only interface ioctls.
///
/// # Safety
///
/// As for [`ifioctl`].
unsafe fn ifioctl_get(cmd: u64, data: *mut u8) -> Result<(), Errno> {
    match cmd {
        // SAFETY: the caller's contract: an `ifconf`.
        SIOCGIFCONF => return unsafe { ifconf(data) },
        SIOCIFGCLONERS => {
            // SAFETY: the caller's contract: an `if_clonereq`.
            return if_clone_list(unsafe { &mut *data.cast::<IfClonereq>() });
        }
        // SAFETY: the caller's contract: an `ifgroupreq`.
        SIOCGIFGMEMB => return unsafe { if_getgroupmembers(data) },
        SIOCGIFGATTR => {
            net_lock_shared();
            // SAFETY: the caller's contract: an `ifgroupreq`.
            let error = unsafe { if_getgroupattribs(data) };
            net_unlock_shared();
            return error;
        }
        // SAFETY: the caller's contract: an `ifgroupreq`.
        SIOCGIFGLIST => return unsafe { if_getgrouplist(data) },
        _ => {}
    }

    // SAFETY: the caller's contract: an `ifreq`.
    let ifr = unsafe { &mut *data.cast::<Ifreq>() };

    // KERNEL_LOCK(): one CPU.
    let Some(ifp) = if_unit(&ifr.ifr_name) else {
        return Err(Errno::ENXIO);
    };

    let mut error = Ok(());
    match cmd {
        SIOCGIFFLAGS => {
            let mut flags = ifp.if_flags.get();
            if ifq_is_oactive(&ifp.if_snd) {
                flags |= IFF_OACTIVE;
            }
            ifr.set_ifr_flags(flags as i16);
        }

        SIOCGIFXFLAGS => {
            ifr.set_ifr_flags((ifp.if_xflags.get() & !(IFXF_MPSAFE | IFXF_CLONED)) as i16);
        }

        SIOCGIFMETRIC => ifr.set_ifr_metric(ifp.if_metric.get() as i32),

        SIOCGIFMTU => ifr.set_ifr_mtu(ifp.if_mtu.get() as i32),

        SIOCGIFHARDMTU => ifr.set_ifr_hardmtu(ifp.if_hardmtu.get() as i32),

        SIOCGIFDATA => {
            let mut ifdata = MaybeUninit::<IfData>::zeroed();
            // SAFETY: all-zero is a valid `if_data` (integers and a timeval).
            let ifdata = unsafe { ifdata.assume_init_mut() };

            net_lock_shared();
            // KERNEL_LOCK(): one CPU.
            if_getdata(ifp, ifdata);
            net_unlock_shared();

            // SAFETY: zero-filled, then written member by member.
            error = copyout(unsafe { bytes_of(ifdata) }, ifr.ifr_data() as usize);
        }

        SIOCGIFDESCR => {
            let mut ifdescrbuf = [0u8; IFDESCRSIZE];
            // KERNEL_LOCK(): one CPU.
            let _ = strlcpy(&mut ifdescrbuf, &ifp.if_description.get());

            error = copyoutstr(&ifdescrbuf, ifr.ifr_data() as usize).map(|_| ());
        }
        SIOCGIFRTLABEL => {
            let rtlabelid = ifp.if_rtlabelid.get();

            let mut ifrtlabelbuf = [0u8; RTLABEL_LEN];
            error = if rtlabelid != 0 && rtlabel_id2name(rtlabelid, &mut ifrtlabelbuf).is_some() {
                copyoutstr(&ifrtlabelbuf, ifr.ifr_data() as usize).map(|_| ())
            } else {
                Err(Errno::ENOENT)
            };
        }
        SIOCGIFPRIORITY => ifr.set_ifr_metric(i32::from(ifp.if_priority.get())),

        SIOCGIFRDOMAIN => ifr.set_ifr_rdomainid(ifp.if_rdomain.get() as i32),

        // SAFETY: the caller's contract: an `ifgroupreq`.
        SIOCGIFGROUP => error = unsafe { if_getgroup(data, ifp) },

        SIOCGIFLLPRIO => ifr.set_ifr_llprio(i32::from(ifp.if_llprio.get())),

        _ => panic(format_args!("invalid ioctl {cmd}")),
    }

    if_put(ifp);

    error
}

/// `if_sffpage_check`.
///
/// # Safety
///
/// `data` points at an `if_sffpage`.
unsafe fn if_sffpage_check(data: *const u8) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let sff = unsafe { &*data.cast::<IfSffpage>() };

    match sff.sff_addr {
        IFSFF_ADDR_EEPROM | IFSFF_ADDR_DDM => Ok(()),
        _ => Err(Errno::EINVAL),
    }
}

/// `if_txhprio_l2_check`: a valid layer 2 transmit header priority setting.
pub fn if_txhprio_l2_check(hdrprio: i32) -> Result<(), Errno> {
    match hdrprio {
        IF_HDRPRIO_PACKET => Ok(()),
        _ if (IF_HDRPRIO_MIN..=IF_HDRPRIO_MAX).contains(&hdrprio) => Ok(()),
        _ => Err(Errno::EINVAL),
    }
}

/// `if_txhprio_l3_check`: a valid layer 3 transmit header priority setting.
pub fn if_txhprio_l3_check(hdrprio: i32) -> Result<(), Errno> {
    match hdrprio {
        IF_HDRPRIO_PACKET | IF_HDRPRIO_PAYLOAD => Ok(()),
        _ if (IF_HDRPRIO_MIN..=IF_HDRPRIO_MAX).contains(&hdrprio) => Ok(()),
        _ => Err(Errno::EINVAL),
    }
}

/// `if_rxhprio_l2_check`: a valid layer 2 receive header priority setting.
pub fn if_rxhprio_l2_check(hdrprio: i32) -> Result<(), Errno> {
    match hdrprio {
        IF_HDRPRIO_PACKET | IF_HDRPRIO_OUTER => Ok(()),
        _ if (IF_HDRPRIO_MIN..=IF_HDRPRIO_MAX).contains(&hdrprio) => Ok(()),
        _ => Err(Errno::EINVAL),
    }
}

/// `if_rxhprio_l3_check`: a valid layer 3 receive header priority setting.
pub fn if_rxhprio_l3_check(hdrprio: i32) -> Result<(), Errno> {
    match hdrprio {
        IF_HDRPRIO_PACKET | IF_HDRPRIO_PAYLOAD | IF_HDRPRIO_OUTER => Ok(()),
        _ if (IF_HDRPRIO_MIN..=IF_HDRPRIO_MAX).contains(&hdrprio) => Ok(()),
        _ => Err(Errno::EINVAL),
    }
}

/// `ifconf`: return interface configuration of system. List may be used in later ioctl's
/// (above) to get other information.
///
/// # Safety
///
/// `data` points at an `ifconf` whose `ifc_req` is a user address.
unsafe fn ifconf(data: *mut u8) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ifc = unsafe { &mut *data.cast::<Ifconf>() };
    let mut space = ifc.ifc_len as isize;
    let ifr_size = size_of::<Ifreq>() as isize;
    let sa_size = size_of::<Sockaddr>();

    // If ifc->ifc_len is 0, fill it in with the needed size and return.
    if space == 0 {
        net_lock_shared();
        for ifp in IFNETLIST.0.iter() {
            if ifp.if_addrlist.is_empty() {
                space += ifr_size;
            } else {
                for ifa in ifp.if_addrlist.iter() {
                    // SAFETY: an interface's addresses are valid sockaddrs.
                    let sa_len = usize::from(unsafe { (*ifa.ifa_addr.get()).sa_len });
                    if sa_len > sa_size {
                        space += (sa_len - sa_size) as isize;
                    }
                    space += ifr_size;
                }
            }
        }
        net_unlock_shared();
        ifc.ifc_len = space as i32;
        return Ok(());
    }

    let if_tmplist: TailqHead<IfTmplist> = TailqHead::new();
    let addr_tmplist: TailqHead<IfaTmplist> = TailqHead::new();

    let mut ifrp = ifc.ifc_req() as usize;
    let mut ifr = Ifreq::zeroed();
    let mut error = Ok(());

    rw_enter_write(&IF_TMPLIST_LOCK);
    net_lock_shared();
    for ifp in IFNETLIST.0.iter() {
        let _ = if_ref(ifp);
        // SAFETY: `if_tmplist` is free under `IF_TMPLIST_LOCK`; the reference keeps the
        // interface alive until it is taken off below.
        unsafe { if_tmplist.insert_tail(ifp) };
    }
    net_unlock_shared();

    'free: for ifp in if_tmplist.iter() {
        if space < ifr_size {
            break 'free;
        }
        ifr.ifr_name = ifp.if_xname.get();

        net_lock_shared();
        for ifa in ifp.if_addrlist.iter() {
            let _ = ifaref(ifa);
            // SAFETY: `ifa_tmplist` is free under `IF_TMPLIST_LOCK`; the reference keeps the
            // address alive until it is taken off below.
            unsafe { addr_tmplist.insert_tail(ifa) };
        }
        net_unlock_shared();

        if addr_tmplist.is_empty() {
            *ifr.ifr_addr_mut() = Sockaddr {
                sa_len: 0,
                sa_family: 0,
                sa_data: [0; 14],
            };
            // SAFETY: an `Ifreq::zeroed` written member by member.
            error = copyout(unsafe { bytes_of(&ifr) }, ifrp);
            if error.is_err() {
                break 'free;
            }

            space -= ifr_size;
            ifrp += ifr_size as usize;
        } else {
            for ifa in addr_tmplist.iter() {
                let sa = ifa.ifa_addr.get();
                // SAFETY: an interface's addresses are valid sockaddrs of `sa_len` bytes.
                let sa_len = usize::from(unsafe { (*sa).sa_len });
                // SAFETY: as above.
                let sa_bytes = unsafe { slice::from_raw_parts(sa.cast::<u8>(), sa_len) };

                if space < ifr_size {
                    break 'free;
                }
                if sa_len <= sa_size {
                    let mut addr = [0u8; 16];
                    addr[..sa_len].copy_from_slice(sa_bytes);
                    // SAFETY: a `Sockaddr` is 16 bytes of plain data.
                    *ifr.ifr_addr_mut() =
                        unsafe { core::mem::transmute::<[u8; 16], Sockaddr>(addr) };
                    // SAFETY: an `Ifreq::zeroed` written member by member.
                    error = copyout(unsafe { bytes_of(&ifr) }, ifrp);
                    if error.is_err() {
                        break 'free;
                    }

                    space -= ifr_size;
                    ifrp += ifr_size as usize;
                } else {
                    let total = (IFNAMSIZ + sa_len) as isize;

                    if space < total {
                        break 'free;
                    }
                    error = copyout(&ifr.ifr_name, ifrp);
                    if error.is_err() {
                        break 'free;
                    }
                    error = copyout(sa_bytes, ifrp + offset_of!(Ifreq, ifr_ifru));
                    if error.is_err() {
                        break 'free;
                    }

                    space -= total;
                    ifrp += total as usize;
                }
            }
        }

        while let Some(ifa) = addr_tmplist.first() {
            // SAFETY: the first element of the temporary list.
            unsafe { addr_tmplist.remove(ifa) };
            ifafree(ifa_static(ifa));
        }
    }

    // free:
    while let Some(ifa) = addr_tmplist.first() {
        // SAFETY: the first element of the temporary list.
        unsafe { addr_tmplist.remove(ifa) };
        ifafree(ifa_static(ifa));
    }
    while let Some(ifp) = if_tmplist.first() {
        // SAFETY: the first element of the temporary list.
        unsafe { if_tmplist.remove(ifp) };
        if_put(ifp);
    }
    rw_exit_write(&IF_TMPLIST_LOCK);

    if error.is_ok() {
        ifc.ifc_len -= space as i32;
    }
    error
}

/// `if_counters_alloc`: the interface's own counters (`counters_alloc(ifc_ncounters)`).
pub fn if_counters_alloc(ifp: &Ifnet) {
    kassert!(ifp.if_counters.get().is_none());

    let Some(c) = malloc(size_of::<IfCounterArray>(), M_COUNTERS, M_WAITOK | M_ZERO) else {
        panic(format_args!("if_counters_alloc: out of memory"));
    };
    // SAFETY: a zero-filled block of the array's size, aligned for it; zero is a valid
    // `AtomicU64`. It lives until `if_counters_free`.
    ifp.if_counters
        .set(Some(unsafe { &*c.as_ptr().cast::<IfCounterArray>() }));
}

/// `if_counters_free`.
pub fn if_counters_free(ifp: &Ifnet) {
    let c = ifp.if_counters.get();
    kassert!(c.is_some());

    if let Some(c) = c {
        free(
            NonNull::from(c).cast(),
            M_COUNTERS,
            size_of::<IfCounterArray>(),
        );
    }
    ifp.if_counters.set(None);
}

/// `if_getdata`: the interface's `if_data` (`SIOCGIFDATA`, the routing socket).
pub fn if_getdata(ifp: &Ifnet, data: &mut IfData) {
    data.ifi_type = ifp.if_type.get();
    data.ifi_addrlen = ifp.if_addrlen.get();
    data.ifi_hdrlen = ifp.if_hdrlen.get();
    data.ifi_link_state = ifp.if_link_state.get();
    data.ifi_mtu = ifp.if_mtu.get();
    data.ifi_metric = ifp.if_metric.get();
    data.ifi_baudrate = ifp.if_baudrate.get();
    data.ifi_capabilities = ifp.if_capabilities.get();
    data.ifi_rdomain = ifp.if_rdomain.get();
    data.ifi_lastchange = ifp.if_lastchange.get();

    let counters: [u64; IFC_NCOUNTERS] = core::array::from_fn(|i| {
        let mut v = ifp.if_data_counters[i].get();
        if let Some(c) = ifp.if_counters.get() {
            v += c[i].load(Ordering::Relaxed);
        }
        v
    });
    data.ifi_ipackets = counters[IfCounters::IfcIpackets as usize];
    data.ifi_ierrors = counters[IfCounters::IfcIerrors as usize];
    data.ifi_opackets = counters[IfCounters::IfcOpackets as usize];
    data.ifi_oerrors = counters[IfCounters::IfcOerrors as usize];
    data.ifi_collisions = counters[IfCounters::IfcCollisions as usize];
    data.ifi_ibytes = counters[IfCounters::IfcIbytes as usize];
    data.ifi_obytes = counters[IfCounters::IfcObytes as usize];
    data.ifi_imcasts = counters[IfCounters::IfcImcasts as usize];
    data.ifi_omcasts = counters[IfCounters::IfcOmcasts as usize];
    data.ifi_iqdrops = counters[IfCounters::IfcIqdrops as usize];
    data.ifi_oqdrops = counters[IfCounters::IfcOqdrops as usize];
    data.ifi_noproto = counters[IfCounters::IfcNoproto as usize];

    for i in 0..ifp.if_nifqs.get() {
        ifq_add_data(ifp.ifq(i), data);
    }

    for i in 0..ifp.if_niqs.get() {
        ifiq_add_data(ifp.ifiq(i), data);
    }
}

/// `if_detached_qstart`: dummy start routine installed during detach (if protocols decide to
/// fiddle with the if during detach).
pub fn if_detached_qstart(ifq: &'static Ifqueue) {
    let _ = ifq_purge(ifq);
}

/// `if_detached_ioctl`: dummy ioctl routine installed during detach.
///
/// # Safety
///
/// None: the arguments are not used (the signature is `if_ioctl`'s).
pub unsafe fn if_detached_ioctl(_ifp: &'static Ifnet, _a: u64, _b: *mut u8) -> Result<(), Errno> {
    Err(Errno::ENODEV)
}

/// `ifgroup_icref`.
fn ifgroup_icref(ifg: &IfgGroup) {
    refcnt_take(&ifg.ifg_tmprefcnt);
}

/// `ifgroup_icrele`: frees the group with its last temporary reference.
fn ifgroup_icrele(ifg: &IfgGroup) {
    if refcnt_rele(&ifg.ifg_tmprefcnt) {
        free(NonNull::from(ifg).cast(), M_IFGROUP, size_of::<IfgGroup>());
    }
}

/// `if_creategroup`: create interface group without members.
pub fn if_creategroup(groupname: &[u8]) -> Option<&'static IfgGroup> {
    let ifg = malloc(size_of::<IfgGroup>(), M_IFGROUP, M_NOWAIT)?.cast::<IfgGroup>();

    let mut name = [0u8; IFNAMSIZ];
    let _ = strlcpy(&mut name, groupname);
    // SAFETY: a fresh, aligned block of the group's size, written once before use.
    unsafe {
        ifg.as_ptr().write(IfgGroup {
            ifg_group: name,
            ifg_refcnt: Cell::new(1),
            ifg_pf_kif: Cell::new(ptr::null_mut()),
            ifg_carp_demoted: Cell::new(0),
            ifg_members: TailqHead::new(),
            ifg_next: TailqEntry::new(),
            ifg_tmprefcnt: Refcnt::new(),
            ifg_tmplist: TailqEntry::new(),
        })
    };
    // SAFETY: initialised above; it lives until `ifgroup_icrele` frees it.
    let ifg: &'static IfgGroup = unsafe { ifg.as_ref() };
    ifg.ifg_members.init();
    refcnt_init(&ifg.ifg_tmprefcnt);
    pfi_attach_ifgroup(ifg);
    // SAFETY: a new group, on no list; it stays until `if_delgroup` takes it off.
    unsafe { IFG_HEAD.0.insert_tail(ifg) };

    Some(ifg)
}

/// `if_addgroup`: add a group to an interface.
pub fn if_addgroup(ifp: &'static Ifnet, groupname: &[u8]) -> Result<(), Errno> {
    let groupname = cstr(groupname);
    let namelen = groupname.len();
    if namelen == 0 || namelen >= IFNAMSIZ || groupname[namelen - 1].is_ascii_digit() {
        return Err(Errno::EINVAL);
    }

    if ifp
        .if_groups
        .iter()
        .any(|ifgl| name_eq(&ifgl.ifgl_group.ifg_group, groupname))
    {
        return Err(Errno::EEXIST);
    }

    let Some(ifgl) = malloc(size_of::<IfgList>(), M_IFGROUP, M_NOWAIT) else {
        return Err(Errno::ENOMEM);
    };

    let Some(ifgm) = malloc(size_of::<IfgMember>(), M_IFGROUP, M_NOWAIT) else {
        free(ifgl, M_IFGROUP, size_of::<IfgList>());
        return Err(Errno::ENOMEM);
    };

    let found = IFG_HEAD
        .0
        .iter()
        .find(|ifg| name_eq(&ifg.ifg_group, groupname));

    let ifg: &'static IfgGroup = match found {
        None => match if_creategroup(groupname) {
            Some(ifg) => ifg,
            None => {
                free(ifgl, M_IFGROUP, size_of::<IfgList>());
                free(ifgm, M_IFGROUP, size_of::<IfgMember>());
                return Err(Errno::ENOMEM);
            }
        },
        Some(ifg) => {
            ifg.ifg_refcnt.set(ifg.ifg_refcnt.get() + 1);
            ifg
        }
    };
    kassert!(ifg.ifg_refcnt.get() != 0);

    let ifgl = ifgl.cast::<IfgList>();
    let ifgm = ifgm.cast::<IfgMember>();
    // SAFETY: fresh, aligned blocks of the right sizes, written once before use.
    let (ifgl, ifgm): (&'static IfgList, &'static IfgMember) = unsafe {
        ifgl.as_ptr().write(IfgList {
            ifgl_group: ifg,
            ifgl_next: TailqEntry::new(),
        });
        ifgm.as_ptr().write(IfgMember {
            ifgm_next: TailqEntry::new(),
            ifgm_ifp: ifp,
        });
        (ifgl.as_ref(), ifgm.as_ref())
    };

    // SAFETY: new elements, on no list; they stay until `if_delgroup` frees them.
    unsafe {
        ifg.ifg_members.insert_tail(ifgm);
        ifp.if_groups.insert_tail(ifgl);
    }

    pfi_group_addmember(groupname);

    Ok(())
}

/// `if_delgroup`: remove a group from an interface.
pub fn if_delgroup(ifp: &Ifnet, groupname: &[u8]) -> Result<(), Errno> {
    let Some(ifgl) = ifp
        .if_groups
        .iter()
        .find(|ifgl| name_eq(&ifgl.ifgl_group.ifg_group, groupname))
    else {
        return Err(Errno::ENOENT);
    };
    // SAFETY: an element of the interface's group list, freed at the end of this function.
    let ifgl: &'static IfgList = unsafe { &*ptr::from_ref(ifgl) };
    let ifg = ifgl.ifgl_group;

    // SAFETY: `ifgl` is on the interface's list.
    unsafe { ifp.if_groups.remove(ifgl) };

    if let Some(ifgm) = ifg
        .ifg_members
        .iter()
        .find(|ifgm| ptr::eq(ifgm.ifgm_ifp, ifp))
    {
        // SAFETY: `ifgm` is on the group's member list.
        unsafe { ifg.ifg_members.remove(ifgm) };
        free(
            NonNull::from(ifgm).cast(),
            M_IFGROUP,
            size_of::<IfgMember>(),
        );
    }

    pfi_group_delmember(groupname);

    kassert!(ifg.ifg_refcnt.get() != 0);
    ifg.ifg_refcnt.set(ifg.ifg_refcnt.get() - 1);
    if ifg.ifg_refcnt.get() == 0 {
        // SAFETY: the group is on `ifg_head`.
        unsafe { IFG_HEAD.0.remove(ifg) };
        pfi_detach_ifgroup(ifg);
        ifgroup_icrele(ifg);
    }

    free(NonNull::from(ifgl).cast(), M_IFGROUP, size_of::<IfgList>());

    Ok(())
}

/// Copies one `ifg_req` naming `name` out to the user array.
fn ifgrq_copyout(name: &[u8], member: bool, ifgp: usize) -> Result<(), Errno> {
    let mut ifgrq = IfgReq {
        ifgrq_ifgrqu: IfgrqIfgrqu {
            ifgrqu_group: [0; IFNAMSIZ],
        },
    };
    let mut buf = [0u8; IFNAMSIZ];
    let _ = strlcpy(&mut buf, name);
    if member {
        ifgrq.ifgrq_ifgrqu.ifgrqu_member = buf;
    } else {
        ifgrq.ifgrq_ifgrqu.ifgrqu_group = buf;
    }
    // SAFETY: an `ifg_req` is sixteen bytes, all written.
    copyout(unsafe { bytes_of(&ifgrq) }, ifgp)
}

/// `if_getgroup`: stores all groups from an interface in memory pointed to by `data`.
///
/// # Safety
///
/// `data` points at an `ifgroupreq` whose `ifgr_groups` is a user address.
unsafe fn if_getgroup(data: *mut u8, ifp: &Ifnet) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ifgr = unsafe { &mut *data.cast::<Ifgroupreq>() };
    let ifg_tmplist: TailqHead<IfgTmplist> = TailqHead::new();
    let req = size_of::<IfgReq>();

    if ifgr.ifgr_len == 0 {
        net_lock_shared();
        for _ in ifp.if_groups.iter() {
            ifgr.ifgr_len += req as u32;
        }
        net_unlock_shared();
        return Ok(());
    }

    let mut len = ifgr.ifgr_len as usize;
    let mut ifgp = ifgr.ifgr_groups() as usize;
    let mut error = Ok(());

    rw_enter_write(&IF_TMPLIST_LOCK);

    net_lock_shared();
    for ifgl in ifp.if_groups.iter() {
        ifgroup_icref(ifgl.ifgl_group);
        // SAFETY: `ifg_tmplist` is free under `IF_TMPLIST_LOCK`; the temporary reference
        // keeps the group alive until it is taken off below.
        unsafe { ifg_tmplist.insert_tail(ifgl.ifgl_group) };
    }
    net_unlock_shared();

    for ifg in ifg_tmplist.iter() {
        if len < req {
            error = Err(Errno::EINVAL);
            break;
        }
        error = ifgrq_copyout(&ifg.ifg_group, false, ifgp);
        if error.is_err() {
            break;
        }
        len -= req;
        ifgp += req;
    }

    while let Some(ifg) = ifg_tmplist.first() {
        // SAFETY: the first element of the temporary list.
        unsafe { ifg_tmplist.remove(ifg) };
        ifgroup_icrele(ifg);
    }

    rw_exit_write(&IF_TMPLIST_LOCK);

    error
}

/// `if_getgroupmembers`: stores all members of a group in memory pointed to by `data`.
///
/// # Safety
///
/// As for `if_getgroup`.
unsafe fn if_getgroupmembers(data: *mut u8) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ifgr = unsafe { &mut *data.cast::<Ifgroupreq>() };
    let if_tmplist: TailqHead<IfTmplist> = TailqHead::new();
    let req = size_of::<IfgReq>();
    let mut error = Ok(());

    rw_enter_write(&IF_TMPLIST_LOCK);
    net_lock_shared();

    let Some(ifg) = IFG_HEAD
        .0
        .iter()
        .find(|ifg| name_eq(&ifg.ifg_group, &ifgr.ifgr_name))
    else {
        net_unlock_shared();
        rw_exit_write(&IF_TMPLIST_LOCK);
        return Err(Errno::ENOENT);
    };

    if ifgr.ifgr_len == 0 {
        for _ in ifg.ifg_members.iter() {
            ifgr.ifgr_len += req as u32;
        }
        net_unlock_shared();
        rw_exit_write(&IF_TMPLIST_LOCK);
        return Ok(());
    }

    for ifgm in ifg.ifg_members.iter() {
        let _ = if_ref(ifgm.ifgm_ifp);
        // SAFETY: `if_tmplist` is free under `IF_TMPLIST_LOCK`; the reference keeps the
        // interface alive until it is taken off below.
        unsafe { if_tmplist.insert_tail(ifgm.ifgm_ifp) };
    }
    net_unlock_shared();

    let mut len = ifgr.ifgr_len as usize;
    let mut ifgp = ifgr.ifgr_groups() as usize;

    for ifp in if_tmplist.iter() {
        if len < req {
            error = Err(Errno::EINVAL);
            break;
        }
        error = ifgrq_copyout(&ifp.if_xname.get(), true, ifgp);
        if error.is_err() {
            break;
        }
        len -= req;
        ifgp += req;
    }

    while let Some(ifp) = if_tmplist.first() {
        // SAFETY: the first element of the temporary list.
        unsafe { if_tmplist.remove(ifp) };
        if_put(ifp);
    }
    rw_exit_write(&IF_TMPLIST_LOCK);

    error
}

/// `if_getgroupattribs`.
///
/// # Safety
///
/// `data` points at an `ifgroupreq`.
unsafe fn if_getgroupattribs(data: *mut u8) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ifgr = unsafe { &mut *data.cast::<Ifgroupreq>() };

    let Some(ifg) = IFG_HEAD
        .0
        .iter()
        .find(|ifg| name_eq(&ifg.ifg_group, &ifgr.ifgr_name))
    else {
        return Err(Errno::ENOENT);
    };

    ifgr.ifgr_ifgru.ifgru_attrib = IfgAttrib {
        ifg_carp_demoted: ifg.ifg_carp_demoted.get(),
    };

    Ok(())
}

/// `if_setgroupattribs`: changes a group's carp demotion and tells its members.
///
/// # Safety
///
/// `data` points at an `ifgroupreq`.
unsafe fn if_setgroupattribs(data: *mut u8) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ifgr = unsafe { &*data.cast::<Ifgroupreq>() };

    let Some(ifg) = IFG_HEAD
        .0
        .iter()
        .find(|ifg| name_eq(&ifg.ifg_group, &ifgr.ifgr_name))
    else {
        return Err(Errno::ENOENT);
    };

    let demote = ifgr.ifgr_attrib().ifg_carp_demoted;
    let sum = demote + ifg.ifg_carp_demoted.get();
    if !(0..=0xff).contains(&sum) {
        return Err(Errno::EINVAL);
    }

    ifg.ifg_carp_demoted.set(sum);

    for ifgm in ifg.ifg_members.iter() {
        // SAFETY: the caller's `ifgroupreq` is what `SIOCSIFGATTR` takes.
        let _ = unsafe { ifp_ioctl(ifgm.ifgm_ifp, SIOCSIFGATTR, data) };
    }

    Ok(())
}

/// `if_getgrouplist`: stores all groups in memory pointed to by `data`.
///
/// # Safety
///
/// As for `if_getgroup`.
unsafe fn if_getgrouplist(data: *mut u8) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ifgr = unsafe { &mut *data.cast::<Ifgroupreq>() };
    let ifg_tmplist: TailqHead<IfgTmplist> = TailqHead::new();
    let req = size_of::<IfgReq>();

    if ifgr.ifgr_len == 0 {
        net_lock_shared();
        for _ in IFG_HEAD.0.iter() {
            ifgr.ifgr_len += req as u32;
        }
        net_unlock_shared();
        return Ok(());
    }

    let mut len = ifgr.ifgr_len as usize;
    let mut ifgp = ifgr.ifgr_groups() as usize;
    let mut error = Ok(());

    rw_enter_write(&IF_TMPLIST_LOCK);

    net_lock_shared();
    for ifg in IFG_HEAD.0.iter() {
        ifgroup_icref(ifg);
        // SAFETY: as in `if_getgroup`.
        unsafe { ifg_tmplist.insert_tail(ifg) };
    }
    net_unlock_shared();

    for ifg in ifg_tmplist.iter() {
        if len < req {
            error = Err(Errno::EINVAL);
            break;
        }
        error = ifgrq_copyout(&ifg.ifg_group, false, ifgp);
        if error.is_err() {
            break;
        }
        len -= req;
        ifgp += req;
    }

    while let Some(ifg) = ifg_tmplist.first() {
        // SAFETY: the first element of the temporary list.
        unsafe { ifg_tmplist.remove(ifg) };
        ifgroup_icrele(ifg);
    }

    rw_exit_write(&IF_TMPLIST_LOCK);

    error
}

/// `if_group_routechange`: a default route changed; rebuild the `egress` group.
///
/// # Safety
///
/// `dst` points at a readable socket address; `mask` is NULL or one.
pub unsafe fn if_group_routechange(dst: *const Sockaddr, mask: *const Sockaddr) {
    // SAFETY: the caller's contract.
    unsafe {
        if (*dst).sa_family == AF_INET
            && (*satosin_const(dst)).sin_addr.s_addr == INADDR_ANY
            && !mask.is_null()
            && ((*mask).sa_len == 0 || (*satosin_const(mask)).sin_addr.s_addr == INADDR_ANY)
        {
            let _ = if_group_egress_build();
        }
    }
    #[cfg(feature = "inet6")]
    // SAFETY: the caller's contract; an AF_INET6 address and its mask are `sockaddr_in6`s,
    // of which only `sin6_addr` is read (unaligned: a `sockaddr` has a smaller alignment).
    unsafe {
        if (*dst).sa_family == AF_INET6
            && in6_are_addr_equal(
                &ptr::read_unaligned(&raw const (*satosin6_const(dst)).sin6_addr),
                &IN6ADDR_ANY,
            )
            && !mask.is_null()
            && ((*mask).sa_len == 0
                || in6_are_addr_equal(
                    &ptr::read_unaligned(&raw const (*satosin6_const(mask)).sin6_addr),
                    &IN6ADDR_ANY,
                ))
        {
            let _ = if_group_egress_build();
        }
    }
}

/// `if_group_egress_build`: the `egress` group is the interfaces of the default routes.
pub fn if_group_egress_build() -> Result<(), Errno> {
    let ifg = IFG_HEAD
        .0
        .iter()
        .find(|ifg| name_eq(&ifg.ifg_group, IFG_EGRESS));

    if let Some(ifg) = ifg {
        for ifgm in ifg.ifg_members.iter() {
            let _ = if_delgroup(ifgm.ifgm_ifp, IFG_EGRESS);
        }
    }

    let mut sa_in = SockaddrIn {
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_family: AF_INET,
        ..SockaddrIn::default()
    };
    // SAFETY: a local `sockaddr_in`, the destination and the mask (`0.0.0.0/0`).
    let mut rt = unsafe {
        rtable_lookup(
            0,
            sintosa(&mut sa_in),
            sintosa(&mut sa_in),
            ptr::null(),
            RTP_ANY,
        )
    };
    while let Some(r) = rt {
        if r.rt_flags.get() & (RTF_REJECT | RTF_BLACKHOLE) == 0
            && let Some(ifp) = if_get(r.rt_ifidx.get())
        {
            let _ = if_addgroup(ifp, IFG_EGRESS);
            if_put(ifp);
        }
        rt = rtable_iterate(r);
    }

    #[cfg(feature = "inet6")]
    {
        let mut sa_in6 = SA6_ANY;
        // SAFETY: a local `sockaddr_in6`, the destination and the mask (`::/0`).
        let mut rt = unsafe {
            rtable_lookup(
                0,
                sin6tosa(&raw mut sa_in6),
                sin6tosa(&raw mut sa_in6),
                ptr::null(),
                RTP_ANY,
            )
        };
        while let Some(r) = rt {
            if r.rt_flags.get() & (RTF_REJECT | RTF_BLACKHOLE) == 0
                && let Some(ifp) = if_get(r.rt_ifidx.get())
            {
                let _ = if_addgroup(ifp, IFG_EGRESS);
                if_put(ifp);
            }
            rt = rtable_iterate(r);
        }
    }

    Ok(())
}

/// `ifpromisc`: set/clear promiscuous mode on interface `ifp` based on the truth value of
/// `pswitch`. The calls are reference counted so that only the first "on" request actually
/// has an effect, as does the final "off" request. Results are undefined if the "off" and
/// "on" requests are not matched.
pub fn ifpromisc(ifp: &'static Ifnet, pswitch: bool) -> Result<(), Errno> {
    net_assert_locked("ifpromisc"); // modifying if_flags and if_pcount

    let oif_flags = ifp.if_flags.get();
    let oif_pcount = ifp.if_pcount.get();
    if pswitch {
        ifp.if_pcount.set(oif_pcount + 1);
        if oif_pcount != 0 {
            return Ok(());
        }
        ifp.if_flags.set(ifp.if_flags.get() | IFF_PROMISC);
    } else {
        ifp.if_pcount.set(oif_pcount - 1);
        if oif_pcount - 1 > 0 {
            return Ok(());
        }
        ifp.if_flags.set(ifp.if_flags.get() & !IFF_PROMISC);
    }

    if ifp.if_flags.get() & IFF_UP == 0 {
        return Ok(());
    }

    let mut ifr = Ifreq::zeroed();
    ifr.set_ifr_flags(ifp.if_flags.get() as i16);
    // SAFETY: `ifr` is a `struct ifreq`, what `SIOCSIFFLAGS` takes.
    let error = unsafe { ifp_ioctl(ifp, SIOCSIFFLAGS, ptr::from_mut(&mut ifr).cast()) };
    if error.is_err() {
        ifp.if_flags.set(oif_flags);
        ifp.if_pcount.set(oif_pcount);
    }

    error
}

/// `ifsetlro`: set/clear LRO flag and restart interface if needed.
pub fn ifsetlro(ifp: &'static Ifnet, on: bool) -> Result<(), Errno> {
    let s = splnet();

    net_assert_locked("ifsetlro"); // for ioctl
    // KERNEL_ASSERT_LOCKED(): one CPU.

    let error = 'out: {
        let mut ifr = Ifreq::zeroed();
        if on {
            ifr.set_ifr_flags(IFXF_LRO as i16);
        }

        // SAFETY: `ifr` is a `struct ifreq`, what `SIOCSIFXFLAGS` takes.
        if unsafe { ifp_ioctl(ifp, SIOCSIFXFLAGS, ptr::from_mut(&mut ifr).cast()) }.is_ok() {
            break 'out Ok(());
        }

        if ifp.if_capabilities.get() & IFCAP_LRO == 0 {
            break 'out Err(Errno::ENOTSUP);
        }

        if on && ifp.if_xflags.get() & IFXF_LRO == 0 {
            // NETHER > 0
            if ifp.if_type.get() == IFT_ETHER && ether_brport_isset(ifp).is_err() {
                break 'out Err(Errno::EBUSY);
            }
            ifp.if_xflags.set(ifp.if_xflags.get() | IFXF_LRO);
        } else if !on && ifp.if_xflags.get() & IFXF_LRO != 0 {
            ifp.if_xflags.set(ifp.if_xflags.get() & !IFXF_LRO);
        }
        Ok(())
    };

    splx(s);

    error
}

/// `ifa_add`: links a protocol's address into the interface's list.
///
/// # Safety
///
/// `ifa` is on no interface's list, its sockaddrs are valid, and it stays valid until
/// `ifa_del` and the last `ifafree`.
pub unsafe fn ifa_add(ifp: &Ifnet, ifa: &'static Ifaddr) {
    net_assert_locked_exclusive("ifa_add");
    // SAFETY: the caller's contract.
    unsafe { ifp.if_addrlist.insert_tail(ifa) };
}

/// `ifa_del`.
pub fn ifa_del(ifp: &Ifnet, ifa: &Ifaddr) {
    net_assert_locked_exclusive("ifa_del");
    kassert!(ifp.if_addrlist.iter().any(|a| ptr::eq(a, ifa)));
    // SAFETY: an address passed to `ifa_del` was added to this interface with `ifa_add`.
    unsafe { ifp.if_addrlist.remove(ifa) };
}

/// `ifa_update_broadaddr`: replaces the broadcast address in place.
///
/// # Safety
///
/// `sa` points at a readable socket address; the address's broadcast address is a valid one.
pub unsafe fn ifa_update_broadaddr(_ifp: &Ifnet, ifa: &Ifaddr, sa: *const Sockaddr) {
    let broad = ifa.ifa_broadaddr().get();
    // SAFETY: the caller's contract.
    unsafe {
        if (*broad).sa_len != (*sa).sa_len {
            panic(format_args!(
                "ifa_update_broadaddr does not support dynamic length"
            ));
        }
        ptr::copy(
            sa.cast::<u8>(),
            broad.cast::<u8>(),
            usize::from((*sa).sa_len),
        );
    }
}

/// `ifa_print_all` (`DDB`): debug function, can be called from ddb>.
pub fn ifa_print_all() {
    for ifp in IFNETLIST.0.iter() {
        for ifa in ifp.if_addrlist.iter() {
            // SAFETY: an interface's addresses are valid sockaddrs.
            match unsafe { (*ifa.ifa_addr.get()).sa_family } {
                AF_INET => {
                    // printf("%s", inet_ntop(AF_INET, &satosin(ifa->ifa_addr)->sin_addr, ...)):
                    // inet_ntop (lib/libkern) is not ported.
                    let _ = unported!("inet_ntop");
                }
                #[cfg(feature = "inet6")]
                AF_INET6 => {
                    // SAFETY: an AF_INET6 address is a `sockaddr_in6`; read unaligned.
                    let a = unsafe {
                        ptr::read_unaligned(
                            &raw const (*satosin6_const(ifa.ifa_addr.get())).sin6_addr,
                        )
                    };
                    printf(format_args!("{}", In6Ntop(a)));
                }
                _ => {}
            }
            printf(format_args!(" on {}\n", Str(&ifp.if_xname.get())));
        }
    }
}

/// `ifnewlladdr`: bounces the interface after its link address changed.
pub fn ifnewlladdr(ifp: &'static Ifnet) {
    #[cfg(feature = "inet6")]
    let i_am_router = IP6_FORWARDING.load(Ordering::Relaxed) != 0;

    net_assert_locked("ifnewlladdr"); // for ioctl and in6
    // KERNEL_ASSERT_LOCKED(): one CPU.

    let mut ifrq = Ifreq::zeroed();
    let up = ifp.if_flags.get() & IFF_UP != 0;
    let mut setflags = |flags: i32| {
        ifp.if_flags.set(flags);
        ifrq.set_ifr_flags(flags as i16);
        // SAFETY: `ifrq` is a `struct ifreq`, what `SIOCSIFFLAGS` takes.
        let _ = unsafe { ifp_ioctl(ifp, SIOCSIFFLAGS, ptr::from_mut(&mut ifrq).cast()) };
    };

    if up {
        // go down for a moment...
        setflags(ifp.if_flags.get() & !IFF_UP);
    }

    setflags(ifp.if_flags.get() | IFF_UP);

    // Update the link-local address. Don't do it if we're a router to avoid confusing hosts
    // on the network.
    #[cfg(feature = "inet6")]
    if !i_am_router && let Some(ia6) = in6ifa_ifpforlinklocal(ifp, 0) {
        in6_purgeaddr(&ia6.ia_ifa);
        if_hooks_run(&ifp.if_addrhooks);
        // The C ignores the result.
        let _ = in6_ifattach(ifp);
    }

    if !up {
        // go back down
        setflags(ifp.if_flags.get() & !IFF_UP);
    }
}

/// `if_addrhook_add`.
///
/// # Safety
///
/// As for [`if_detachhook_add`].
pub unsafe fn if_addrhook_add(ifp: &Ifnet, t: &Task) {
    mtx_enter(&IF_HOOKS_MTX);
    // SAFETY: the caller's contract.
    unsafe { ifp.if_addrhooks.insert_tail(t) };
    mtx_leave(&IF_HOOKS_MTX);
}

/// `if_addrhook_del`.
///
/// # Safety
///
/// `t` was added with `if_addrhook_add` on this interface.
pub unsafe fn if_addrhook_del(ifp: &Ifnet, t: &Task) {
    mtx_enter(&IF_HOOKS_MTX);
    // SAFETY: the caller's contract.
    unsafe { ifp.if_addrhooks.remove(t) };
    mtx_leave(&IF_HOOKS_MTX);
}

/// `if_addrhooks_run`.
pub fn if_addrhooks_run(ifp: &Ifnet) {
    if_hooks_run(&ifp.if_addrhooks);
}

/// `if_rxr_init`: a receive ring accounting with watermarks `lwm` and `hwm`.
pub fn if_rxr_init(rxr: &mut IfRxring, lwm: u32, hwm: u32) {
    *rxr = IfRxring::default();

    rxr.rxr_adjusted = TICKS.load(Ordering::Relaxed);
    rxr.rxr_lwm = lwm;
    rxr.rxr_cwm = lwm;
    rxr.rxr_hwm = hwm;
}

/// `if_rxr_adjust_cwm`.
fn if_rxr_adjust_cwm(rxr: &mut IfRxring) {
    if rxr.rxr_alive >= rxr.rxr_lwm {
        return;
    } else if rxr.rxr_cwm < rxr.rxr_hwm {
        rxr.rxr_cwm += 1;
    }

    rxr.rxr_adjusted = TICKS.load(Ordering::Relaxed);
}

/// `if_rxr_livelocked`: the system is too busy; shrink the ring's current watermark.
pub fn if_rxr_livelocked(rxr: &mut IfRxring) {
    let ticks = TICKS.load(Ordering::Relaxed);
    if ticks.wrapping_sub(rxr.rxr_adjusted) >= 1 {
        if rxr.rxr_cwm > rxr.rxr_lwm {
            rxr.rxr_cwm -= 1;
        }

        rxr.rxr_adjusted = ticks;
    }
}

/// `if_rxr_get`: how many of `max` slots the driver may fill now.
pub fn if_rxr_get(rxr: &mut IfRxring, max: u32) -> u32 {
    if TICKS.load(Ordering::Relaxed).wrapping_sub(rxr.rxr_adjusted) >= 1 {
        // we're free to try for an adjustment
        if_rxr_adjust_cwm(rxr);
    }

    if rxr.rxr_alive >= rxr.rxr_cwm {
        return 0;
    }

    let diff = min(rxr.rxr_cwm - rxr.rxr_alive, max);
    rxr.rxr_alive += diff;

    diff
}

/// `if_rxr_info_ioctl`: copies `e` out for `SIOCGIFRXR`; `uifri` is the user's
/// `if_rxrinfo`.
pub fn if_rxr_info_ioctl(uifri: usize, t: u32, e: &[IfRxringInfo]) -> Result<(), Errno> {
    let mut kifri = MaybeUninit::<IfRxrinfo>::zeroed();
    // SAFETY: an `if_rxrinfo` is an integer and a pointer, valid for any bytes; the slice is
    // the structure's bytes.
    let kifri = unsafe {
        let bytes =
            slice::from_raw_parts_mut(kifri.as_mut_ptr().cast::<u8>(), size_of::<IfRxrinfo>());
        copyin(uifri, bytes)?;
        kifri.assume_init_mut()
    };

    let n = min(t, kifri.ifri_total) as usize;
    kifri.ifri_total = t;

    if n > 0 {
        // SAFETY: `if_rxring_info` has no padding (16 + 4 + 20 bytes of integers).
        let bytes = unsafe {
            slice::from_raw_parts(
                e.as_ptr().cast::<u8>(),
                size_of::<IfRxringInfo>() * n.min(e.len()),
            )
        };
        copyout(bytes, kifri.ifri_entries as usize)?;
    }

    // SAFETY: zero-filled, then written member by member.
    copyout(unsafe { bytes_of(kifri) }, uifri)
}

/// `if_rxr_ioctl`: `SIOCGIFRXR` for a driver with one ring.
pub fn if_rxr_ioctl(
    ifri: usize,
    name: Option<&[u8]>,
    size: u32,
    rxr: &IfRxring,
) -> Result<(), Errno> {
    let mut ifr = IfRxringInfo {
        ifr_name: [0; 16],
        ifr_size: size,
        ifr_info: *rxr,
    };

    if let Some(name) = name {
        let _ = strlcpy(&mut ifr.ifr_name, name);
    }

    if_rxr_info_ioctl(ifri, 1, slice::from_ref(&ifr))
}

/// `niq_enqueue`: network stack input queues: queues the packet and schedules its software
/// interrupt; `true` (the C's nonzero) when the queue was full and the packet dropped.
pub fn niq_enqueue(niq: &Niqueue, m: &'static Mbuf) -> bool {
    let rv = mq_enqueue(&niq.ni_q, m);
    if !rv {
        schednetisr(niq.ni_isr);
    } else {
        if_congestion();
    }

    rv
}

/// `unhandled_af`: panics on an address family the caller cannot handle.
pub fn unhandled_af(af: i32) -> ! {
    panic(format_args!("unhandled af {af}"))
}

/// `softnet_count`: the softnet task queues in use, one per CPU up to `NET_TASKQ`.
pub fn softnet_count() -> u32 {
    let n = NSOFTNETS.load(Ordering::Relaxed);
    if n != 0 {
        return n;
    }
    let n = min(
        NET_TASKQ as u32,
        NCPUS.load(Ordering::Relaxed).max(1) as u32,
    );
    NSOFTNETS.store(n, Ordering::Relaxed);
    n
}

/// `net_sn`: the softnet of an interface index (or queue index).
pub fn net_sn(ifindex: u32) -> &'static Softnet {
    &SOFTNETS[(ifindex % softnet_count()) as usize]
}

/// `net_tq`: the softnet task queue of an interface index; NULL before `softnet_init`.
pub fn net_tq(ifindex: u32) -> Option<&'static Taskq> {
    net_sn(ifindex).sn_taskq.get()
}

/// The barrier task of `net_tq_barriers`: `refcnt_rele_wake` on its argument.
fn net_tq_barrier_rele(arg: *mut c_void) {
    // SAFETY: `net_tq_barriers` armed the task with its `refcnt`, which it keeps alive until
    // `refcnt_finalize` has seen every barrier run.
    refcnt_rele_wake(unsafe { &*arg.cast::<Refcnt>() });
}

/// `net_tq_barriers`: waits until every softnet task queue has run what was queued before.
pub fn net_tq_barriers(wmesg: &'static str) {
    let barriers: [Task; NET_TASKQ] = [const { Task::zeroed() }; NET_TASKQ];
    let r = Refcnt::new();

    for (i, barrier) in barriers.iter().enumerate().take(softnet_count() as usize) {
        task_set(
            barrier,
            net_tq_barrier_rele,
            ptr::from_ref(&r).cast_mut().cast(),
        );
        refcnt_take(&r);
        let Some(tq) = SOFTNETS[i].sn_taskq.get() else {
            refcnt_rele_wake(&r);
            continue;
        };
        // SAFETY: the task lives on this stack until `refcnt_finalize` returns, which is
        // after the task queue has run it (the task's run is the reference's release).
        let barrier: &'static Task = unsafe { &*ptr::from_ref(barrier) };
        let _ = task_add(tq, barrier);
    }

    refcnt_finalize(&r, wmesg);
}

// LP64 sizes of the C structures.
const _: () = {
    assert!(size_of::<IfNameindex>() == 16);
    assert!(size_of::<IfClonereq>() == 16);
    assert!(size_of::<IfRxring>() == 20);
    assert!(size_of::<IfRxringInfo>() == 40);
    assert!(size_of::<IfRxrinfo>() == 16);
    assert!(size_of::<IfData>() == 144);
    assert!(size_of::<IfMsghdr>() == 168);
    assert!(size_of::<IfaMsghdr>() == 24);
    assert!(size_of::<IfAnnouncemsghdr>() == 26);
    assert!(size_of::<IfIeee80211Data>() == 52);
    assert!(size_of::<IfIeee80211Msghdr>() == 64);
    assert!(size_of::<IfNameindexMsg>() == 20);
    assert!(size_of::<IfgReq>() == 16);
    assert!(size_of::<Ifgroupreq>() == 40);
    assert!(size_of::<Ifreq>() == 32);
    assert!(size_of::<Ifaliasreq>() == 64);
    assert!(size_of::<Ifmediareq>() == 64);
    assert!(size_of::<Ifkalivereq>() == 24);
    assert!(size_of::<Ifconf>() == 16);
    assert!(size_of::<IfLaddrreq>() == 536);
    assert!(size_of::<IfAfreq>() == 17);
    assert!(size_of::<IfParent>() == 32);
    assert!(size_of::<IfSffpage>() == 274);
};

#[cfg(test)]
pub(crate) mod tests;
