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
 * Copyright (c) 2012-2013 Henning Brauer <henning@openbsd.org>
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
/* </LICENSES> */

/* <CODE> */
//! The kernel's network interface: `<net/if_var.h>`, `struct ifnet` and its addresses.
//!
//! Upstream: sys/net/if_var.h @ 3ce1f3f79392
//!
//! Each interface accepts output datagrams of a specified maximum length, and provides higher
//! level routines with input datagrams received from its medium. Output occurs when the
//! routine `if_output` is called, with four parameters: `(*ifp->if_output)(ifp, m, dst, rt)`,
//! where `m` is the mbuf chain to be sent and `dst` is the destination address. The output
//! routine encapsulates the supplied datagram if necessary, and then transmits it on its
//! medium. On input, each interface unwraps the data received by it, and either places it on
//! the input queue of an internetwork datagram routine and posts the associated software
//! interrupt, or passes the datagram to a raw packet input routine. The routines live in
//! `net/if.c` (`net/if_.rs`) and `net/route.c`.
//!
//! The locks named in the field docs are the C's: \[I\] immutable after creation, \[d\] the
//! driver's, \[c\] ioctl or routing socket context (kernel lock), \[K\] kernel lock, \[N\] net
//! lock, \[T\] `if_tmplist_lock`, \[m\] `if_maddrlock`.
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - `struct ifnet` is shared through `&'static Ifnet`, as `struct ifnet *` is: an interface
//!   lives in its driver's softc or in a `malloc` block and goes away only after `if_detach`
//!   and `refcnt_finalize`. Its members are `Cell`s (the C changes them through the pointer
//!   under the locks above), so the all-zero `Ifnet` is valid, as the C's `M_ZERO` softcs
//!   need.
//! - The function pointers are `Option<fn>` with Rust signatures: errors are
//!   `Result<(), Errno>`, `if_ioctl`'s `caddr_t data` is a raw pointer to the request (the
//!   handlers are `unsafe fn`s: the buffer's type is the command's), `if_output` takes the
//!   destination as `*const Sockaddr` (a sockaddr may be longer than `struct sockaddr`) and is
//!   `unsafe` for the same reason, and `if_bpf_mtap` answers `bool` (the C's "drop it").
//! - `if_flags` is an `int`-wide cell (`unsigned short` in C): the `IFF_*` constants of
//!   `<net/if.h>` are `i32`, and every value the stack stores fits sixteen bits (`ifioctl`
//!   masks what `ifr_flags` brings, as the C's assignment truncates).
//! - The `if_carp_ptr` union is two fields side by side (`docs/C_TO_RUST.md`): `if_carp`, the
//!   `SMR_LIST_HEAD` of `carp(4)` (not configured, kept as its first pointer), and
//!   `if_carpdevidx`. The `ifq_ifqs`/`ifiq_ifiqs` one-element maps likewise (`net/ifq.rs`).
//! - `if_counters` (`struct cpumem *`, `<sys/percpu.h>`, not ported) is one array of atomics
//!   per interface, [`IfCounterArray`]: the CPUs share it instead of each having its own
//!   copy, which costs contention but not correctness. `counters_inc` and `counters_pkt`
//!   for it are in `net/if_.rs`, where `if_counters_alloc`/`if_counters_free` make and free
//!   it.
//! - `enum if_counters` is [`IfCounters`] (`ifc_ipackets` is `IfCounters::IfcIpackets`, as
//!   `enum mbstat_counters` is in `<sys/mbuf.h>`); the `if_ipackets` .. `if_noproto` shorthands
//!   are methods returning the counter's `Cell`.
//! - One member that the C does not have: a flag set by `ether_ifattach`, which receives the
//!   `struct arpcom`, so that the C's `(struct arpcom *)ifp` cast ([`Ifnet::is_arpcom`],
//!   `netinet/if_ether.rs`) is checked instead of trusted.
//! - `struct rtentry *` is `Option<&'static Rtentry>` (`net/route.rs`); a route's reference
//!   is taken with `rtref` and dropped with `rtfree`, as in C.
//! - `ifnetlist`, `if_tmplist_lock` and the functions are defined in `net/if_.rs`, where
//!   `net/if.c` defines them; `if_input_process_proto` panics on a packet without an input
//!   function, where the C would call through NULL.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{mq_delist, mq_dequeue, sysctl_mq};
use crate::machine::intr::IPL_NET;
use crate::net::if_::{IFDESCRSIZE, IFNAMSIZ, IfRxring};
use crate::net::if_dl::SockaddrDl;
use crate::net::ifq::{Ifiqueue, Ifqueue};
use crate::net::route::{Route, Rtentry};
use crate::netinet6::nd6::NdIfinfo;
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::mbuf::{Mbuf, MbufList, MbufQueue, mq_drops, mq_len};
use crate::sys::queue::{ListEntry, TailqEntry, TailqHead};
use crate::sys::refcnt::Refcnt;
use crate::sys::rwlock::Rwlock;
use crate::sys::socket::Sockaddr;
use crate::sys::task::{Task, TaskList};
use crate::sys::time::Timeval;
use crate::sys::timeout::Timeout;

pub use crate::net::if_::{IF_TMPLIST_LOCK, IFNETLIST};

/// `IFNET_SLOWTIMO`: granularity is 1 second.
pub const IFNET_SLOWTIMO: i32 = 1;

/// `IF_TXMIT_MIN`.
pub const IF_TXMIT_MIN: u32 = 1;
/// `IF_TXMIT_DEFAULT`.
pub const IF_TXMIT_DEFAULT: u32 = 16;

// Default interface priorities.

/// `IF_WIRED_DEFAULT_PRIORITY`.
pub const IF_WIRED_DEFAULT_PRIORITY: u8 = 0;
/// `IF_WIRELESS_DEFAULT_PRIORITY`.
pub const IF_WIRELESS_DEFAULT_PRIORITY: u8 = 4;
/// `IF_WWAN_DEFAULT_PRIORITY`.
pub const IF_WWAN_DEFAULT_PRIORITY: u8 = 6;
/// `IF_CARP_DEFAULT_PRIORITY`.
pub const IF_CARP_DEFAULT_PRIORITY: u8 = 15;

/// `IFA_ROUTE`: auto-magically installed route.
pub const IFA_ROUTE: u32 = 0x01;

/// `struct netstack`: the packet lists a softnet thread works through (`if_input_process`).
pub struct Netstack {
    /// `ns_input`: packets to give to an interface's `if_input` again (`if_vinput`).
    pub ns_input: MbufList,
    /// `ns_proto`: packets for a protocol's input function (`if_input_proto`).
    pub ns_proto: MbufList,
    /// `ns_route`: the route cache of the packets this softnet thread delivers
    /// (`ip_input_if`).
    pub ns_route: Route,
    /// `ns_tcp_ml`: TCP segments gathered for `tcp_input_mlist`.
    pub ns_tcp_ml: MbufList,
    /// `ns_tcp6_ml`: the same for IPv6.
    pub ns_tcp6_ml: MbufList,
}

impl Netstack {
    /// Four empty lists.
    pub const fn new() -> Self {
        Self {
            ns_input: MbufList::new(),
            ns_proto: MbufList::new(),
            ns_route: Route::new(),
            ns_tcp_ml: MbufList::new(),
            ns_tcp6_ml: MbufList::new(),
        }
    }
}

impl Default for Netstack {
    fn default() -> Self {
        Self::new()
    }
}

/// `int (*ifc_create)(struct if_clone *, int)`: creates unit `unit` of a cloner.
pub type IfcCreateFn = fn(&'static IfClone, i32) -> Result<(), Errno>;
/// `int (*ifc_destroy)(struct ifnet *)`: destroys a cloned interface.
pub type IfcDestroyFn = fn(&'static Ifnet) -> Result<(), Errno>;

/// `struct if_clone`: a "cloning" interface.
pub struct IfClone {
    /// \[I\] `ifc_list`: on list of cloners.
    pub ifc_list: ListEntry<IfClone>,
    /// `ifc_name`: name of device, e.g. `gif` (no NUL).
    pub ifc_name: &'static [u8],
    /// `ifc_namelen`: length of name.
    pub ifc_namelen: usize,
    /// `ifc_create`.
    pub ifc_create: IfcCreateFn,
    /// `ifc_destroy`: NULL when the cloner's interfaces cannot be destroyed.
    pub ifc_destroy: Option<IfcDestroyFn>,
}

// SAFETY: `ifc_list` is written only by `if_clone_attach`, while `main` attaches the
// pseudo-devices before the application processors run; the cloner list is immutable
// afterwards.
unsafe impl Sync for IfClone {}

impl IfClone {
    /// `IF_CLONE_INITIALIZER(name, create, destroy)`.
    pub const fn new(
        name: &'static [u8],
        create: IfcCreateFn,
        destroy: Option<IfcDestroyFn>,
    ) -> Self {
        Self {
            ifc_list: ListEntry::new(),
            ifc_name: name,
            ifc_namelen: name.len(),
            ifc_create: create,
            ifc_destroy: destroy,
        }
    }
}

queue_adapter!(
    /// `LIST_ENTRY(if_clone) ifc_list`: `if_cloners`.
    pub IfClones: IfClone, ifc_list => ListEntry<IfClone>
);

/// `enum if_counters`: the interface statistics, indices into `if_data_counters` and
/// `if_counters`.
#[repr(usize)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IfCounters {
    /// `ifc_ipackets`: packets received on interface.
    IfcIpackets,
    /// `ifc_ierrors`: input errors on interface.
    IfcIerrors,
    /// `ifc_opackets`: packets sent on interface.
    IfcOpackets,
    /// `ifc_oerrors`: output errors on interface.
    IfcOerrors,
    /// `ifc_collisions`: collisions on csma interfaces.
    IfcCollisions,
    /// `ifc_ibytes`: total number of octets received.
    IfcIbytes,
    /// `ifc_obytes`: total number of octets sent.
    IfcObytes,
    /// `ifc_imcasts`: packets received via multicast.
    IfcImcasts,
    /// `ifc_omcasts`: packets sent via multicast.
    IfcOmcasts,
    /// `ifc_iqdrops`: dropped on input, this interface.
    IfcIqdrops,
    /// `ifc_oqdrops`: dropped on output, this interface.
    IfcOqdrops,
    /// `ifc_noproto`: destined for unsupported protocol.
    IfcNoproto,
    /// `ifc_ncounters`.
    IfcNcounters,
}

/// `ifc_ncounters`.
pub const IFC_NCOUNTERS: usize = IfCounters::IfcNcounters as usize;

/// What `counters_alloc(ifc_ncounters)` returns here: one counter per `if_counters`, bumped
/// with relaxed atomics (see the module's deviations).
pub type IfCounterArray = [AtomicU64; IFC_NCOUNTERS];

/// `void (*if_rtrequest)(struct ifnet *, int, struct rtentry *)`: check or clean routes.
pub type IfRtrequestFn = fn(&'static Ifnet, i32, Option<&'static Rtentry>);
/// `void (*if_input)(struct ifnet *, struct mbuf *, struct netstack *)`; the netstack is NULL
/// when the caller holds the net lock and wants the packet handled at once.
pub type IfInputFn = fn(&'static Ifnet, &'static Mbuf, Option<&Netstack>);
/// `int (*if_bpf_mtap)(caddr_t, const struct mbuf *, u_int)`: `true` drops the packet.
pub type IfBpfMtapFn = fn(*mut u8, &Mbuf, u32) -> bool;
/// `int (*if_output)(struct ifnet *, struct mbuf *, struct sockaddr *, struct rtentry *)`.
///
/// # Safety
///
/// `dst` points at a readable socket address of at least `sa_len` bytes (and of the type its
/// `sa_family` names), valid for the call.
pub type IfOutputFn = unsafe fn(
    &'static Ifnet,
    &'static Mbuf,
    *const Sockaddr,
    Option<&'static Rtentry>,
) -> Result<(), Errno>;
/// `int (*if_enqueue)(struct ifnet *, struct mbuf *)`.
pub type IfEnqueueFn = fn(&'static Ifnet, &'static Mbuf) -> Result<(), Errno>;
/// `void (*if_start)(struct ifnet *)`: initiate output.
pub type IfStartFn = fn(&'static Ifnet);
/// `int (*if_ioctl)(struct ifnet *, u_long, caddr_t)`: ioctl hook.
///
/// # Safety
///
/// `data` points at a kernel buffer, aligned for and holding the argument structure the
/// command encodes (`IOCPARM_LEN(cmd)` bytes: a `struct ifreq` for `SIOCSIFFLAGS`, ...), which
/// the handler may read and write for the duration of the call.
pub type IfIoctlFn = unsafe fn(&'static Ifnet, u64, *mut u8) -> Result<(), Errno>;
/// `void (*if_watchdog)(struct ifnet *)`: timer routine.
pub type IfWatchdogFn = fn(&'static Ifnet);
/// `int (*if_wol)(struct ifnet *, int)`: WoL routine.
pub type IfWolFn = fn(&'static Ifnet, bool) -> Result<(), Errno>;
/// `void (*if_qstart)(struct ifqueue *)`.
pub type IfQstartFn = fn(&'static Ifqueue);

/// `struct ifnet`: a network interface, providing a packet transport mechanism (ala level 0
/// of the PUP protocols). The all-zero value is valid (see the module's deviations).
pub struct Ifnet {
    /// \[I\] `if_softc`: lower-level data for this if.
    pub if_softc: Cell<*mut c_void>,
    /// `if_refcnt`.
    pub if_refcnt: Refcnt,
    /// \[NK\] `if_list`: all struct ifnets are chained.
    pub if_list: TailqEntry<Ifnet>,
    /// \[T\] `if_tmplist`: temporary list.
    pub if_tmplist: TailqEntry<Ifnet>,
    /// \[N\] `if_addrlist`: list of addresses per if.
    pub if_addrlist: TailqHead<IfAddrlist>,
    /// \[m\] `if_maddrlist`: list of multicast records.
    pub if_maddrlist: TailqHead<IfMaddrlist>,
    /// \[N\] `if_groups`: list of groups per if.
    pub if_groups: TailqHead<IfGroups>,
    /// `if_maddrlock`.
    pub if_maddrlock: Rwlock,
    /// \[I\] `if_addrhooks`: address change callbacks.
    pub if_addrhooks: TailqHead<TaskList>,
    /// \[I\] `if_linkstatehooks`: link change callbacks.
    pub if_linkstatehooks: TailqHead<TaskList>,
    /// \[I\] `if_detachhooks`: detach callbacks.
    pub if_detachhooks: TailqHead<TaskList>,
    /// \[I\] `if_rtrequest`: check or clean routes (+ or -)'d.
    pub if_rtrequest: Cell<Option<IfRtrequestFn>>,
    /// \[I\] `if_xname`: external name (name + unit), NUL-terminated.
    pub if_xname: Cell<[u8; IFNAMSIZ]>,
    /// \[N\] `if_pcount`: # of promiscuous listeners.
    pub if_pcount: Cell<i32>,
    /// \[K\] `if_bridgeidx`: used by bridge ports.
    pub if_bridgeidx: Cell<u32>,
    /// `if_bpf`: packet filter structure.
    pub if_bpf: Cell<*mut u8>,
    /// `if_mcast`: used by multicast code (`struct vif *`).
    pub if_mcast: Cell<*mut c_void>,
    /// `if_mcast6`: used by IPv6 multicast code (`struct mif6 *`).
    pub if_mcast6: Cell<*mut c_void>,
    /// `if_pf_kif`: pf interface abstraction.
    pub if_pf_kif: Cell<*mut u8>,
    /// `if_carp` (`if_carp_ptr.carp_s`): carp if list (used by `IFT_ETHER`), its first
    /// pointer.
    pub if_carp: Cell<*mut c_void>,
    /// `if_carpdevidx` (`if_carp_ptr.carp_idx`): index of carpdev (used by `IFT_CARP`).
    pub if_carpdevidx: Cell<u32>,
    /// \[I\] `if_index`: unique index for this if.
    pub if_index: Cell<u32>,
    /// `if_timer`: time 'til `if_watchdog` called.
    pub if_timer: Cell<i16>,
    /// \[N\] `if_flags`: up/down, broadcast, etc. (`IFF_*`).
    pub if_flags: Cell<i32>,
    /// \[N\] `if_xflags`: extra softnet flags (`IFXF_*`).
    pub if_xflags: Cell<i32>,

    // Stats and other data about if. Should be in sync with if_data.
    /// `if_type`: `IFT_*`.
    pub if_type: Cell<u8>,
    /// `if_addrlen`.
    pub if_addrlen: Cell<u8>,
    /// `if_hdrlen`.
    pub if_hdrlen: Cell<u8>,
    /// `if_link_state`.
    pub if_link_state: Cell<u8>,
    /// `if_mtu`.
    pub if_mtu: Cell<u32>,
    /// `if_metric`.
    pub if_metric: Cell<u32>,
    /// `if_baudrate`.
    pub if_baudrate: Cell<u64>,
    /// `if_capabilities`: `IFCAP_*`.
    pub if_capabilities: Cell<u32>,
    /// `if_rdomain`.
    pub if_rdomain: Cell<u32>,
    /// \[c\] `if_lastchange`: last op. state change.
    pub if_lastchange: Cell<Timeval>,
    /// `if_data_counters`.
    pub if_data_counters: [Cell<u64>; IFC_NCOUNTERS],

    /// `if_counters`: per cpu stats (NULL until `if_counters_alloc`).
    pub if_counters: Cell<Option<&'static IfCounterArray>>,
    /// \[d\] `if_hardmtu`: maximum MTU device supports.
    pub if_hardmtu: Cell<u32>,
    /// \[c\] `if_description`: interface description.
    pub if_description: Cell<[u8; IFDESCRSIZE]>,
    /// \[c\] `if_rtlabelid`: next route label.
    pub if_rtlabelid: Cell<u16>,
    /// \[c\] `if_priority`: route priority offset.
    pub if_priority: Cell<u8>,
    /// \[N\] `if_llprio`: link layer priority.
    pub if_llprio: Cell<u8>,
    /// \[I\] `if_slowtimo`: watchdog timeout.
    pub if_slowtimo: Timeout,
    /// \[I\] `if_watchdogtask`: watchdog task.
    pub if_watchdogtask: Task,
    /// \[I\] `if_linkstatetask`: task to do route updates.
    pub if_linkstatetask: Task,

    // procedure handles
    /// `if_input`.
    pub if_input: Cell<Option<IfInputFn>>,
    /// `if_bpf_mtap`.
    pub if_bpf_mtap: Cell<Option<IfBpfMtapFn>>,
    /// `if_output`: output routine (enqueue).
    pub if_output: Cell<Option<IfOutputFn>>,
    /// `if_ll_output`: link level output function.
    pub if_ll_output: Cell<Option<IfOutputFn>>,
    /// `if_enqueue`.
    pub if_enqueue: Cell<Option<IfEnqueueFn>>,
    /// `if_start`: initiate output.
    pub if_start: Cell<Option<IfStartFn>>,
    /// `if_ioctl`: ioctl hook.
    pub if_ioctl: Cell<Option<IfIoctlFn>>,
    /// `if_watchdog`: timer routine.
    pub if_watchdog: Cell<Option<IfWatchdogFn>>,
    /// `if_wol`: WoL routine.
    pub if_wol: Cell<Option<IfWolFn>>,

    // queues
    /// `if_snd`: transmit queue.
    pub if_snd: Ifqueue,
    /// \[I\] `if_ifqs`: pointer to an array of `if_nifqs` sndqs.
    pub if_ifqs: Cell<*const Cell<*const Ifqueue>>,
    /// `if_qstart`.
    pub if_qstart: Cell<Option<IfQstartFn>>,
    /// \[I\] `if_nifqs`: number of output queues.
    pub if_nifqs: Cell<u32>,
    /// \[c\] `if_txmit`: txmitigation amount.
    pub if_txmit: Cell<u32>,

    /// `if_rcv`: rx/input queue.
    pub if_rcv: Ifiqueue,
    /// \[I\] `if_iqs`: pointer to the array of `if_niqs` iqs.
    pub if_iqs: Cell<*const Cell<*const Ifiqueue>>,
    /// \[I\] `if_niqs`: number of input queues.
    pub if_niqs: Cell<u32>,

    /// \[N\] `if_sadl`: pointer to our `sockaddr_dl` (`malloc`ed, `sdl_len` bytes).
    pub if_sadl: Cell<*mut SockaddrDl>,

    /// \[I\] `if_nd`: IPv6 Neighbor Discovery info (`struct nd_ifinfo *`, `nd6_ifattach`'s
    /// allocation; `netinet6/nd6.rs`'s `if_nd` reads it).
    pub if_nd: Cell<Option<NonNull<NdIfinfo>>>,

    /// Not in the C: set by `ether_ifattach`, the interface is the `ac_if` of a `struct
    /// arpcom` (see the module's deviations).
    if_arpcom: AtomicBool,
}

// SAFETY: the members change under the locks their docs name (the net lock, the kernel lock,
// the driver's), as in C; the queues and the reference count synchronise themselves.
unsafe impl Sync for Ifnet {}

/// Defines the `if_ipackets` .. `if_noproto` shorthands for `if_data_counters[ifc_*]`.
macro_rules! if_data_counter {
    ($($(#[$doc:meta])* $name:ident => $idx:ident;)*) => {
        $(
            $(#[$doc])*
            pub fn $name(&self) -> &Cell<u64> {
                &self.if_data_counters[IfCounters::$idx as usize]
            }
        )*
    };
}

impl Ifnet {
    if_data_counter! {
        /// `if_ipackets`.
        if_ipackets => IfcIpackets;
        /// `if_ierrors`.
        if_ierrors => IfcIerrors;
        /// `if_opackets`.
        if_opackets => IfcOpackets;
        /// `if_oerrors`.
        if_oerrors => IfcOerrors;
        /// `if_collisions`.
        if_collisions => IfcCollisions;
        /// `if_ibytes`.
        if_ibytes => IfcIbytes;
        /// `if_obytes`.
        if_obytes => IfcObytes;
        /// `if_imcasts`.
        if_imcasts => IfcImcasts;
        /// `if_omcasts`.
        if_omcasts => IfcOmcasts;
        /// `if_iqdrops`.
        if_iqdrops => IfcIqdrops;
        /// `if_oqdrops`.
        if_oqdrops => IfcOqdrops;
        /// `if_noproto`.
        if_noproto => IfcNoproto;
    }

    /// `ifp->if_ifqs[i]`: send queue `i`, below `if_nifqs`.
    pub fn ifq(&self, i: u32) -> &'static Ifqueue {
        assert_index(i, self.if_nifqs.get());
        // SAFETY: `if_attach_common` points `if_ifqs` at the one-element map inside `if_snd`
        // and `if_attach_queues` at a `mallocarray` of `if_nifqs` slots, each filled with a
        // live queue before the count is published; the maps and queues stay until
        // `if_detach`, after which nobody holds the interface.
        unsafe { &*(*self.if_ifqs.get().add(i as usize)).get() }
    }

    /// `ifp->if_iqs[i]`: input queue `i`, below `if_niqs`.
    pub fn ifiq(&self, i: u32) -> &'static Ifiqueue {
        assert_index(i, self.if_niqs.get());
        // SAFETY: as for `ifq`, with `if_rcv`'s map and `if_attach_iqueues`.
        unsafe { &*(*self.if_iqs.get().add(i as usize)).get() }
    }

    /// Whether `ether_ifattach` made this interface the `ac_if` of a `struct arpcom`.
    pub fn is_arpcom(&self) -> bool {
        self.if_arpcom.load(Ordering::Relaxed)
    }

    /// Records that this interface is the `ac_if` of a `struct arpcom`.
    ///
    /// # Safety
    ///
    /// `self` is the `ac_if` member of an `Arpcom` (`netinet/if_ether.rs`), which lives as
    /// long as the interface.
    pub unsafe fn set_arpcom(&self) {
        self.if_arpcom.store(true, Ordering::Relaxed);
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(ifnet_head, ifnet)` through `if_list`: `ifnetlist`.
    pub IfnetList: Ifnet, if_list => TailqEntry<Ifnet>
);

queue_adapter!(
    /// `TAILQ_ENTRY(ifnet) if_tmplist`: the temporary lists of `ifconf` and
    /// `if_getgroupmembers`.
    pub IfTmplist: Ifnet, if_tmplist => TailqEntry<Ifnet>
);

/// `struct ifnet_head`, made `Sync`: `ifnetlist` is changed with both the kernel and the net
/// lock held and read with either (`net/if.c`).
pub struct IfnetHead(pub TailqHead<IfnetList>);

// SAFETY: see the type's doc; the head's cells are only touched under those locks.
unsafe impl Sync for IfnetHead {}

/// `struct ifaddr`: information about one address of an interface. They are maintained by the
/// different address families, are allocated and attached when an address is set, and are
/// linked together so all addresses for an interface can be located. A protocol's address
/// (`struct in_ifaddr`) embeds it first, hence `#[repr(C)]`.
#[repr(C)]
pub struct Ifaddr {
    /// `ifa_addr`: address of interface.
    pub ifa_addr: Cell<*mut Sockaddr>,
    /// `ifa_dstaddr`: other end of p-to-p link (`ifa_broadaddr`: broadcast address).
    pub ifa_dstaddr: Cell<*mut Sockaddr>,
    /// `ifa_netmask`: used to determine subnet.
    pub ifa_netmask: Cell<*mut Sockaddr>,
    /// `ifa_ifp`: back-pointer to interface.
    pub ifa_ifp: Cell<Option<&'static Ifnet>>,
    /// \[N\] `ifa_list`: list of addresses for interface.
    pub ifa_list: TailqEntry<Ifaddr>,
    /// \[T\] `ifa_tmplist`: temporary list.
    pub ifa_tmplist: TailqEntry<Ifaddr>,
    /// `ifa_flags`: interface flags, see below.
    pub ifa_flags: Cell<u32>,
    /// `ifa_refcnt`: number of `rt_ifa` references.
    pub ifa_refcnt: Refcnt,
    /// `ifa_metric`: cost of going out this interface.
    pub ifa_metric: Cell<i32>,
}

// SAFETY: the members change under the net lock, as in C.
unsafe impl Sync for Ifaddr {}

impl Ifaddr {
    /// `ifa_broadaddr`: broadcast address interface (the `ifa_dstaddr` member).
    pub fn ifa_broadaddr(&self) -> &Cell<*mut Sockaddr> {
        &self.ifa_dstaddr
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(, ifaddr) if_addrlist`.
    pub IfAddrlist: Ifaddr, ifa_list => TailqEntry<Ifaddr>
);

queue_adapter!(
    /// `TAILQ_ENTRY(ifaddr) ifa_tmplist`: `ifconf`'s temporary list.
    pub IfaTmplist: Ifaddr, ifa_tmplist => TailqEntry<Ifaddr>
);

/// `struct ifmaddr`: interface multicast address. A protocol's record (`struct in_multi`)
/// embeds it first, hence `#[repr(C)]`.
#[repr(C)]
pub struct Ifmaddr {
    /// \[m\] `ifma_list`: per-interface list.
    pub ifma_list: TailqEntry<Ifmaddr>,
    /// \[I\] `ifma_addr`: protocol address.
    pub ifma_addr: Cell<*mut Sockaddr>,
    /// `ifma_refcnt`: count of references.
    pub ifma_refcnt: Refcnt,
    /// \[I\] `ifma_ifidx`: index of the interface.
    pub ifma_ifidx: Cell<u32>,
}

queue_adapter!(
    /// `TAILQ_HEAD(, ifmaddr) if_maddrlist`.
    pub IfMaddrlist: Ifmaddr, ifma_list => TailqEntry<Ifmaddr>
);

/// `struct ifg_group`: an interface group.
pub struct IfgGroup {
    /// \[I\] `ifg_group`: group name, NUL-terminated.
    pub ifg_group: [u8; IFNAMSIZ],
    /// \[N\] `ifg_refcnt`: group reference count.
    pub ifg_refcnt: Cell<u32>,
    /// \[I\] `ifg_pf_kif`: pf interface group.
    pub ifg_pf_kif: Cell<*mut u8>,
    /// \[K\] `ifg_carp_demoted`: carp demotion counter.
    pub ifg_carp_demoted: Cell<i32>,
    /// \[N\] `ifg_members`: list of members per group.
    pub ifg_members: TailqHead<IfgMembers>,
    /// \[N\] `ifg_next`: all groups are chained.
    pub ifg_next: TailqEntry<IfgGroup>,

    /// `ifg_tmprefcnt`.
    pub ifg_tmprefcnt: Refcnt,
    /// \[T\] `ifg_tmplist`: temporary list.
    pub ifg_tmplist: TailqEntry<IfgGroup>,
}

queue_adapter!(
    /// `TAILQ_HEAD(, ifg_group)` through `ifg_next`: `ifg_head`.
    pub IfgHead: IfgGroup, ifg_next => TailqEntry<IfgGroup>
);

queue_adapter!(
    /// `TAILQ_HEAD(, ifg_group)` through `ifg_tmplist`: the temporary lists.
    pub IfgTmplist: IfgGroup, ifg_tmplist => TailqEntry<IfgGroup>
);

/// `struct ifg_member`: an interface in a group.
pub struct IfgMember {
    /// \[N\] `ifgm_next`: all members are chained.
    pub ifgm_next: TailqEntry<IfgMember>,
    /// \[I\] `ifgm_ifp`: member interface.
    pub ifgm_ifp: &'static Ifnet,
}

queue_adapter!(
    /// `TAILQ_HEAD(, ifg_member) ifg_members`.
    pub IfgMembers: IfgMember, ifgm_next => TailqEntry<IfgMember>
);

/// `struct ifg_list`: a group of an interface.
pub struct IfgList {
    /// \[I\] `ifgl_group`: interface group.
    pub ifgl_group: &'static IfgGroup,
    /// \[N\] `ifgl_next`: all groups are chained.
    pub ifgl_next: TailqEntry<IfgList>,
}

queue_adapter!(
    /// `TAILQ_HEAD(, ifg_list) if_groups`.
    pub IfGroups: IfgList, ifgl_next => TailqEntry<IfgList>
);

/// `struct niqueue`: a network stack input queue.
pub struct Niqueue {
    /// `ni_q`.
    pub ni_q: MbufQueue,
    /// `ni_isr`: the `NETISR_*` bit to schedule.
    pub ni_isr: i32,
}

impl Niqueue {
    /// `NIQUEUE_INITIALIZER(len, isr)`.
    pub const fn new(len: u32, isr: i32) -> Self {
        Self {
            ni_q: MbufQueue::new(len, IPL_NET),
            ni_isr: isr,
        }
    }
}

/// The bound check of `if_ifqs[i]`/`if_iqs[i]`, which the C leaves to its callers.
fn assert_index(i: u32, n: u32) {
    if i >= n {
        panic(format_args!("interface queue {i} of {n}"));
    }
}

/// `niq_dequeue(q)`.
pub fn niq_dequeue(q: &Niqueue) -> Option<&'static Mbuf> {
    mq_dequeue(&q.ni_q)
}

/// `niq_delist(q, ml)`.
pub fn niq_delist(q: &Niqueue, ml: &MbufList) {
    mq_delist(&q.ni_q, ml);
}

/// `niq_len(q)`.
pub fn niq_len(q: &Niqueue) -> u32 {
    mq_len(&q.ni_q)
}

/// `niq_drops(q)`.
pub fn niq_drops(q: &Niqueue) -> u32 {
    mq_drops(&q.ni_q)
}

/// `sysctl_niq(n, l, op, olp, np, nl, niq)`.
pub fn sysctl_niq(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    niq: &Niqueue,
) -> Result<(), Errno> {
    sysctl_mq(name, oldp, oldlenp, newp, newlen, &niq.ni_q)
}

/// `if_input_process_proto`: a helper for `if_input_process` and similar functions; calls
/// the protocol input function `if_input_proto` stored in the packet's `ph_cookie`.
pub fn if_input_process_proto(ifp: &'static Ifnet, m: &'static Mbuf, ns: Option<&Netstack>) {
    let cookie = m.m_pkthdr().ph_cookie.get();
    if cookie.is_null() {
        panic(format_args!("if_input_process_proto: no input function"));
    }
    // SAFETY: `if_input_proto`, the only writer of `ph_cookie` on the `ns_proto` path, stored
    // an `IfInputFn` there; a function pointer and `*mut ()` have the same size, and this
    // transmute is the inverse of that cast.
    let input = unsafe { core::mem::transmute::<*mut (), IfInputFn>(cookie) };
    input(ifp, m, ns);
}

/// `if_rxr_put(r, c)`: `c` slots of the ring were consumed.
pub fn if_rxr_put(r: &mut IfRxring, c: u32) {
    r.rxr_alive -= c;
}

/// `if_rxr_needrefill(r)`: the ring is below its low watermark.
pub fn if_rxr_needrefill(r: &IfRxring) -> bool {
    r.rxr_alive < r.rxr_lwm
}

/// `if_rxr_inuse(r)`.
pub fn if_rxr_inuse(r: &IfRxring) -> u32 {
    r.rxr_alive
}

/// `if_rxr_cwm(r)`.
pub fn if_rxr_cwm(r: &IfRxring) -> u32 {
    r.rxr_cwm
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reftest::{assert_complete, assert_defines};

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/net/if_var.h");
        let ours = assert_defines!(defs;
            IF_TXMIT_MIN, IF_TXMIT_DEFAULT, IF_WIRED_DEFAULT_PRIORITY,
            IF_WIRELESS_DEFAULT_PRIORITY, IF_WWAN_DEFAULT_PRIORITY, IF_CARP_DEFAULT_PRIORITY);
        assert_complete(&defs, "IF_", &ours);
        assert_defines!(defs; IFNET_SLOWTIMO, IFA_ROUTE);
        assert_eq!(IFC_NCOUNTERS, 12);
    }
}
/* </TESTS> */
