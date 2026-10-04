/*	$OpenBSD: if_pflow.h,v 1.24 2025/11/13 17:12:30 chris Exp $	*/
/*	$OpenBSD: if_pflow.c,v 1.112 2025/11/13 17:12:30 chris Exp $	*/
/* <LICENSES> */

/*
 * Copyright (c) 2008 Henning Brauer <henning@openbsd.org>
 * Copyright (c) 2008 Joerg Goltermann <jg@osn.de>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF MIND, USE, DATA OR PROFITS, WHETHER IN
 * AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT
 * OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

/*
 * Copyright (c) 2011 Florian Obser <florian@narrans.de>
 * Copyright (c) 2011 Sebastian Benoit <benoit-lists@fb12.de>
 * Copyright (c) 2008 Henning Brauer <henning@openbsd.org>
 * Copyright (c) 2008 Joerg Goltermann <jg@osn.de>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF MIND, USE, DATA OR PROFITS, WHETHER IN
 * AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT
 * OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

//! The flow export interface, `pflow(4)`: `<net/if_pflow.h>` and `net/if_pflow.c`. When pf
//! removes a state created by a rule with the `pflow` option (`PFSTATE_PFLOW`), it hands the
//! state to `export_pflow`, which turns it into two flow records (one per direction) on every
//! `pflowN` interface. The records collect in an mbuf per interface and leave, in UDP
//! datagrams, from `flowsrc` to the collector at `flowdst` (`ifconfig pflow0 flowsrc ...
//! flowdst ...`): NetFlow version 5, or IPFIX (version 10, `pflowproto 10`) with its
//! templates sent at start-up and every `PFLOW_TMPL_TIMEOUT` seconds. A buffer goes out when
//! it is full or `PFLOW_TIMEOUT` seconds after its first record.
//!
//! Upstream: sys/net/if_pflow.h @ 3ce1f3f79392
//! Upstream: sys/net/if_pflow.c @ 3ce1f3f79392
//!
//! The record and header structures are the wire format (`__packed`), `#[repr(C, packed)]`
//! here and copied into the mbufs as bytes with `m_copyback`. The softc is `malloc(M_ZERO)`ed,
//! so the all-zero value of each member is valid; members changed through the shared softc
//! are `Cell`s under the locks the header names (`sc_mtx`, `sc_lock`).
//!
//! ## Deviations
//! - The header and the file share this module.
//! - SMR (`kern/kern_smr.c`) is not ported (see `net/if_.rs`): `pflowif_list` is a plain list
//!   changed under the kernel lock, read without one; `smr_barrier` is empty.
//! - `pflow_counters` (a `cpumem`) is a static array of atomics (`docs/C_TO_RUST.md`).
//! - `sc_flowsrc` and `sc_flowdst` are `Option<SockaddrStorage>`s holding the address (only
//!   its `sa_len` bytes are meaningful) instead of `malloc`ed `struct sockaddr`s, so the
//!   `ENOMEM` paths of `pflow_set` are gone.
//! - The `extern struct pflow_softc *pflowif` of the header is declared and never defined or
//!   used by the C; it is left out.
//! - `export_pflow_if` copies the whole state (`bcopy(st, &pfs_copy, ...)`) only to change its
//!   byte counters; here `pflow_pack_flow` and `copy_flow_data` take the counters as an
//!   argument. Everything else they read is the state's.
//! - `pflow_get_mbuf` takes the softc by reference: the C's `sc == NULL` branch (an empty mbuf
//!   for no interface) has no caller.
//! - The headers the C writes through `mtod()` in `pflow_sendout_v5` and
//!   `pflow_sendout_ipfix` are read out of the mbuf, updated and written back
//!   (`m_copydata`, `m_copyback`).
//! - `PFLOWDEBUG` is not defined: `DPRINTF` is not ported.
//! - `pflowioctl`, `pflow_output` are `unsafe fn`s, the signatures of `if_ioctl` and
//!   `if_output`; `pflowvalidsockaddr` answers `bool`; the senders, `copy_flow_*_to_m` and the
//!   ioctls return `Result`.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_prot::suser;
use crate::kern::kern_rwlock::{
    rw_assert_anylock, rw_assert_wrlock, rw_enter_read, rw_enter_write, rw_exit_read,
    rw_exit_write, rw_init,
};
use crate::kern::kern_sysctl::sysctl_struct;
use crate::kern::kern_task::{task_add, task_set, taskq_del_barrier};
use crate::kern::kern_tc::{getnanotime, gettime, getuptime};
use crate::kern::kern_timeout::{timeout_add_sec, timeout_del, timeout_set_proc};
use crate::kern::subr_prf::{panic, snprintf};
use crate::kern::uipc_mbuf::{
    m_copyback, m_copydata, m_free, m_freem, m_get, m_gethdr, m_prepend, ml_dequeue, mq_delist,
    mq_enqueue, mq_init, mq_purge,
};
use crate::kern::uipc_socket::{sobind, soclose, socreate, sosend};
use crate::kern::uipc_socket2::{solock, sounlock};
use crate::machine::copy::{AbiPod, copyin_obj, copyout_obj};
use crate::machine::cpu::curproc;
use crate::machine::intr::{IPL_MPFLOOR, IPL_SOFTNET};
use crate::net::if_::{
    IFF_RUNNING, IFF_UP, IFNAMSIZ, IFXF_CLONED, Ifreq, counters_pkt, if_alloc_sadl, if_attach,
    if_clone_attach, if_counters_alloc, if_detach, net_tq, unhandled_af,
};
use crate::net::if_types::IFT_PFLOW;
use crate::net::if_var::{IfClone, IfCounters, Ifnet};
use crate::net::pfvar::{PF_IN, PF_OUT, PF_SK_STACK, PF_SK_WIRE};
use crate::net::pfvar_priv::{PfGlobal, PfState, PfStateKey};
use crate::net::route::Rtentry;
use crate::netinet::if_ether::ETHERMTU;
use crate::netinet::in_::{INADDR_ANY, SockaddrIn};
use crate::netinet::udp_var::Udpiphdr;
use crate::netinet6::in6::{In6Addr, SockaddrIn6, in6_is_addr_unspecified};
use crate::sys::endian::{htobe64, htonl, htons};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_WAITOK, M_ZERO};
use crate::sys::mbuf::{
    M_DONTWAIT, M_EXT, M_WAIT, MCLBYTES, MT_DATA, MT_SONAME, Mbuf, MbufList, MbufQueue, mclget,
};
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::queue::{SlistEntry, SlistHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::socket::{
    AF_INET, AF_INET6, MSG_DONTWAIT, NET_PFLOW_STATS, SOCK_DGRAM, Sockaddr, SockaddrStorage,
};
use crate::sys::socketvar::Socket;
use crate::sys::sockio::{
    SIOCGETPFLOW, SIOCSETPFLOW, SIOCSIFADDR, SIOCSIFDSTADDR, SIOCSIFFLAGS, SIOCSIFMTU,
};
use crate::sys::systm::{net_lock, net_unlock};
use crate::sys::task::Task;
use crate::sys::timeout::Timeout;
use crate::sys::types::SaFamily;

/// `PFLOW_ID_LEN`.
pub const PFLOW_ID_LEN: usize = size_of::<u64>();

/// `PFLOW_MAXFLOWS`.
pub const PFLOW_MAXFLOWS: u32 = 30;
/// `PFLOW_ENGINE_TYPE`.
pub const PFLOW_ENGINE_TYPE: u8 = 42;
/// `PFLOW_ENGINE_ID`.
pub const PFLOW_ENGINE_ID: u8 = 42;
/// `PFLOW_MAXBYTES`.
pub const PFLOW_MAXBYTES: u64 = 0xffff_ffff;
/// `PFLOW_TIMEOUT`.
pub const PFLOW_TIMEOUT: i32 = 30;
/// `PFLOW_TMPL_TIMEOUT`: rfc 5101 10.3.6 (p.40) recommends 600.
pub const PFLOW_TMPL_TIMEOUT: i32 = 30;

/// `PFLOW_IPFIX_TMPL_SET_ID`.
pub const PFLOW_IPFIX_TMPL_SET_ID: u16 = 2;

// RFC 5102 Information Element Identifiers

/// `PFIX_IE_octetDeltaCount`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_octetDeltaCount: u16 = 1;
/// `PFIX_IE_packetDeltaCount`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_packetDeltaCount: u16 = 2;
/// `PFIX_IE_protocolIdentifier`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_protocolIdentifier: u16 = 4;
/// `PFIX_IE_ipClassOfService`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_ipClassOfService: u16 = 5;
/// `PFIX_IE_sourceTransportPort`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_sourceTransportPort: u16 = 7;
/// `PFIX_IE_sourceIPv4Address`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_sourceIPv4Address: u16 = 8;
/// `PFIX_IE_ingressInterface`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_ingressInterface: u16 = 10;
/// `PFIX_IE_destinationTransportPort`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_destinationTransportPort: u16 = 11;
/// `PFIX_IE_destinationIPv4Address`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_destinationIPv4Address: u16 = 12;
/// `PFIX_IE_egressInterface`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_egressInterface: u16 = 14;
/// `PFIX_IE_flowEndSysUpTime`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_flowEndSysUpTime: u16 = 21;
/// `PFIX_IE_flowStartSysUpTime`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_flowStartSysUpTime: u16 = 22;
/// `PFIX_IE_sourceIPv6Address`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_sourceIPv6Address: u16 = 27;
/// `PFIX_IE_destinationIPv6Address`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_destinationIPv6Address: u16 = 28;
/// `PFIX_IE_flowStartMilliseconds`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_flowStartMilliseconds: u16 = 152;
/// `PFIX_IE_flowEndMilliseconds`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_flowEndMilliseconds: u16 = 153;
/// `PFIX_IE_postNATSourceIPv4Address`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_postNATSourceIPv4Address: u16 = 225;
/// `PFIX_IE_postNATDestinationIPv4Address`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_postNATDestinationIPv4Address: u16 = 226;
/// `PFIX_IE_postNAPTSourceTransportPort`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_postNAPTSourceTransportPort: u16 = 227;
/// `PFIX_IE_postNAPTDestinationTransportPort`.
#[allow(non_upper_case_globals)] // the C name
pub const PFIX_IE_postNAPTDestinationTransportPort: u16 = 228;

/// `struct pflow_flow` (`__packed`): a NetFlow version 5 record.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PflowFlow {
    /// `src_ip`.
    pub src_ip: u32,
    /// `dest_ip`.
    pub dest_ip: u32,
    /// `nexthop_ip`.
    pub nexthop_ip: u32,
    /// `if_index_in`.
    pub if_index_in: u16,
    /// `if_index_out`.
    pub if_index_out: u16,
    /// `flow_packets`.
    pub flow_packets: u32,
    /// `flow_octets`.
    pub flow_octets: u32,
    /// `flow_start`.
    pub flow_start: u32,
    /// `flow_finish`.
    pub flow_finish: u32,
    /// `src_port`.
    pub src_port: u16,
    /// `dest_port`.
    pub dest_port: u16,
    /// `pad1`.
    pub pad1: u8,
    /// `tcp_flags`.
    pub tcp_flags: u8,
    /// `protocol`.
    pub protocol: u8,
    /// `tos`.
    pub tos: u8,
    /// `src_as`.
    pub src_as: u16,
    /// `dest_as`.
    pub dest_as: u16,
    /// `src_mask`.
    pub src_mask: u8,
    /// `dest_mask`.
    pub dest_mask: u8,
    /// `pad2`.
    pub pad2: u16,
}

/// `struct pflow_set_header` (`__packed`).
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PflowSetHeader {
    /// `set_id`.
    pub set_id: u16,
    /// `set_length`: total length of the set, in octets, including the set header.
    pub set_length: u16,
}

/// `PFLOW_SET_HDRLEN`.
pub const PFLOW_SET_HDRLEN: usize = size_of::<PflowSetHeader>();

/// `struct pflow_tmpl_hdr` (`__packed`).
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PflowTmplHdr {
    /// `tmpl_id`.
    pub tmpl_id: u16,
    /// `field_count`.
    pub field_count: u16,
}

/// `struct pflow_tmpl_fspec` (`__packed`).
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PflowTmplFspec {
    /// `field_id`.
    pub field_id: u16,
    /// `len`.
    pub len: u16,
}

/// `struct pflow_ipfix_tmpl_ipv4` (`__packed`): update `pflow_clone_create` when changing it.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PflowIpfixTmplIpv4 {
    /// `h`.
    pub h: PflowTmplHdr,
    /// `src_ip`.
    pub src_ip: PflowTmplFspec,
    /// `dest_ip`.
    pub dest_ip: PflowTmplFspec,
    /// `if_index_in`.
    pub if_index_in: PflowTmplFspec,
    /// `if_index_out`.
    pub if_index_out: PflowTmplFspec,
    /// `packets`.
    pub packets: PflowTmplFspec,
    /// `octets`.
    pub octets: PflowTmplFspec,
    /// `start`.
    pub start: PflowTmplFspec,
    /// `finish`.
    pub finish: PflowTmplFspec,
    /// `src_port`.
    pub src_port: PflowTmplFspec,
    /// `dest_port`.
    pub dest_port: PflowTmplFspec,
    /// `tos`.
    pub tos: PflowTmplFspec,
    /// `protocol`.
    pub protocol: PflowTmplFspec,
}

/// `PFLOW_IPFIX_TMPL_IPV4_FIELD_COUNT`.
pub const PFLOW_IPFIX_TMPL_IPV4_FIELD_COUNT: u16 = 12;
/// `PFLOW_IPFIX_TMPL_IPV4_ID`.
pub const PFLOW_IPFIX_TMPL_IPV4_ID: u16 = 256;

/// `struct pflow_ipfix_tmpl_nat_ipv4` (`__packed`).
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PflowIpfixTmplNatIpv4 {
    /// `h`.
    pub h: PflowTmplHdr,
    /// `src_ip`.
    pub src_ip: PflowTmplFspec,
    /// `dest_ip`.
    pub dest_ip: PflowTmplFspec,
    /// `if_index_in`.
    pub if_index_in: PflowTmplFspec,
    /// `if_index_out`.
    pub if_index_out: PflowTmplFspec,
    /// `packets`.
    pub packets: PflowTmplFspec,
    /// `octets`.
    pub octets: PflowTmplFspec,
    /// `start`.
    pub start: PflowTmplFspec,
    /// `finish`.
    pub finish: PflowTmplFspec,
    /// `post_src_ip`.
    pub post_src_ip: PflowTmplFspec,
    /// `post_dest_ip`.
    pub post_dest_ip: PflowTmplFspec,
    /// `post_src_port`.
    pub post_src_port: PflowTmplFspec,
    /// `post_dest_port`.
    pub post_dest_port: PflowTmplFspec,
    /// `src_port`.
    pub src_port: PflowTmplFspec,
    /// `dest_port`.
    pub dest_port: PflowTmplFspec,
    /// `tos`.
    pub tos: PflowTmplFspec,
    /// `protocol`.
    pub protocol: PflowTmplFspec,
}

/// `PFLOW_IPFIX_TMPL_NAT_IPV4_FIELD_COUNT`.
pub const PFLOW_IPFIX_TMPL_NAT_IPV4_FIELD_COUNT: u16 = 16;
/// `PFLOW_IPFIX_TMPL_NAT_IPV4_ID`.
pub const PFLOW_IPFIX_TMPL_NAT_IPV4_ID: u16 = 257;

/// `struct pflow_ipfix_tmpl_ipv6` (`__packed`): update `pflow_clone_create` when changing it.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PflowIpfixTmplIpv6 {
    /// `h`.
    pub h: PflowTmplHdr,
    /// `src_ip`.
    pub src_ip: PflowTmplFspec,
    /// `dest_ip`.
    pub dest_ip: PflowTmplFspec,
    /// `if_index_in`.
    pub if_index_in: PflowTmplFspec,
    /// `if_index_out`.
    pub if_index_out: PflowTmplFspec,
    /// `packets`.
    pub packets: PflowTmplFspec,
    /// `octets`.
    pub octets: PflowTmplFspec,
    /// `start`.
    pub start: PflowTmplFspec,
    /// `finish`.
    pub finish: PflowTmplFspec,
    /// `src_port`.
    pub src_port: PflowTmplFspec,
    /// `dest_port`.
    pub dest_port: PflowTmplFspec,
    /// `tos`.
    pub tos: PflowTmplFspec,
    /// `protocol`.
    pub protocol: PflowTmplFspec,
}

/// `PFLOW_IPFIX_TMPL_IPV6_FIELD_COUNT`.
pub const PFLOW_IPFIX_TMPL_IPV6_FIELD_COUNT: u16 = 12;
/// `PFLOW_IPFIX_TMPL_IPV6_ID`.
pub const PFLOW_IPFIX_TMPL_IPV6_ID: u16 = 258;

/// `struct pflow_ipfix_tmpl` (`__packed`): the template set.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PflowIpfixTmpl {
    /// `set_header`.
    pub set_header: PflowSetHeader,
    /// `ipv4_tmpl`.
    pub ipv4_tmpl: PflowIpfixTmplIpv4,
    /// `ipv4_nat_tmpl`.
    pub ipv4_nat_tmpl: PflowIpfixTmplNatIpv4,
    /// `ipv6_tmpl`.
    pub ipv6_tmpl: PflowIpfixTmplIpv6,
}

/// `struct pflow_ipfix_flow4` (`__packed`).
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PflowIpfixFlow4 {
    /// `src_ip`: sourceIPv4Address.
    pub src_ip: u32,
    /// `dest_ip`: destinationIPv4Address.
    pub dest_ip: u32,
    /// `if_index_in`: ingressInterface.
    pub if_index_in: u32,
    /// `if_index_out`: egressInterface.
    pub if_index_out: u32,
    /// `flow_packets`: packetDeltaCount.
    pub flow_packets: u64,
    /// `flow_octets`: octetDeltaCount.
    pub flow_octets: u64,
    /// `flow_start`: flowStartMilliseconds.
    pub flow_start: i64,
    /// `flow_finish`: flowEndMilliseconds.
    pub flow_finish: i64,
    /// `src_port`: sourceTransportPort.
    pub src_port: u16,
    /// `dest_port`: destinationTransportPort.
    pub dest_port: u16,
    /// `tos`: ipClassOfService.
    pub tos: u8,
    /// `protocol`: protocolIdentifier.
    pub protocol: u8,
    // XXX padding needed?
}

/// `struct pflow_ipfix_nat_flow4` (`__packed`).
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PflowIpfixNatFlow4 {
    /// `src_ip`: sourceIPv4Address.
    pub src_ip: u32,
    /// `dest_ip`: destinationIPv4Address.
    pub dest_ip: u32,
    /// `if_index_in`: ingressInterface.
    pub if_index_in: u32,
    /// `if_index_out`: egressInterface.
    pub if_index_out: u32,
    /// `flow_packets`: packetDeltaCount.
    pub flow_packets: u64,
    /// `flow_octets`: octetDeltaCount.
    pub flow_octets: u64,
    /// `flow_start`: flowStartMilliseconds.
    pub flow_start: i64,
    /// `flow_finish`: flowEndMilliseconds.
    pub flow_finish: i64,
    /// `post_src_ip`: postNATSourceIPv4Address.
    pub post_src_ip: u32,
    /// `post_dest_ip`: postNATDestinationIPv4Address.
    pub post_dest_ip: u32,
    /// `post_src_port`: postNAPTSourceTransportPort.
    pub post_src_port: u16,
    /// `post_dest_port`: postNAPTDestinationTransportPort.
    pub post_dest_port: u16,
    /// `src_port`: sourceTransportPort.
    pub src_port: u16,
    /// `dest_port`: destinationTransportPort.
    pub dest_port: u16,
    /// `tos`: ipClassOfService.
    pub tos: u8,
    /// `protocol`: protocolIdentifier.
    pub protocol: u8,
    // XXX padding needed?
}

/// `struct pflow_ipfix_flow6` (`__packed`).
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PflowIpfixFlow6 {
    /// `src_ip`: sourceIPv6Address.
    pub src_ip: In6Addr,
    /// `dest_ip`: destinationIPv6Address.
    pub dest_ip: In6Addr,
    /// `if_index_in`: ingressInterface.
    pub if_index_in: u32,
    /// `if_index_out`: egressInterface.
    pub if_index_out: u32,
    /// `flow_packets`: packetDeltaCount.
    pub flow_packets: u64,
    /// `flow_octets`: octetDeltaCount.
    pub flow_octets: u64,
    /// `flow_start`: flowStartMilliseconds.
    pub flow_start: i64,
    /// `flow_finish`: flowEndMilliseconds.
    pub flow_finish: i64,
    /// `src_port`: sourceTransportPort.
    pub src_port: u16,
    /// `dest_port`: destinationTransportPort.
    pub dest_port: u16,
    /// `tos`: ipClassOfService.
    pub tos: u8,
    /// `protocol`: protocolIdentifier.
    pub protocol: u8,
    // XXX padding needed?
}

/// `struct pflow_header` (`__packed`): the NetFlow version 5 header.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PflowHeader {
    /// `version`.
    pub version: u16,
    /// `count`.
    pub count: u16,
    /// `uptime_ms`.
    pub uptime_ms: u32,
    /// `time_sec`.
    pub time_sec: u32,
    /// `time_nanosec`.
    pub time_nanosec: u32,
    /// `flow_sequence`.
    pub flow_sequence: u32,
    /// `engine_type`.
    pub engine_type: u8,
    /// `engine_id`.
    pub engine_id: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `reserved2`.
    pub reserved2: u8,
}

/// `PFLOW_HDRLEN`.
pub const PFLOW_HDRLEN: usize = size_of::<PflowHeader>();

/// `struct pflow_v10_header` (`__packed`): the IPFIX message header.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PflowV10Header {
    /// `version`.
    pub version: u16,
    /// `length`.
    pub length: u16,
    /// `time_sec`.
    pub time_sec: u32,
    /// `flow_sequence`.
    pub flow_sequence: u32,
    /// `observation_dom`.
    pub observation_dom: u32,
}

/// `PFLOW_IPFIX_HDRLEN`.
pub const PFLOW_IPFIX_HDRLEN: usize = size_of::<PflowV10Header>();

// SAFETY: packed integers and arrays of integers: no padding, any bit pattern valid.
unsafe impl AbiPod for PflowFlow {}
// SAFETY: as above.
unsafe impl AbiPod for PflowSetHeader {}
// SAFETY: as above; the template structures are packed fspecs.
unsafe impl AbiPod for PflowIpfixTmpl {}
// SAFETY: as above.
unsafe impl AbiPod for PflowIpfixFlow4 {}
// SAFETY: as above.
unsafe impl AbiPod for PflowIpfixNatFlow4 {}
// SAFETY: as above.
unsafe impl AbiPod for PflowIpfixFlow6 {}
// SAFETY: as above.
unsafe impl AbiPod for PflowHeader {}
// SAFETY: as above.
unsafe impl AbiPod for PflowV10Header {}

/// `struct pflowstats`: `net.pflow.stats`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Pflowstats {
    /// `pflow_flows`.
    pub pflow_flows: u64,
    /// `pflow_packets`.
    pub pflow_packets: u64,
    /// `pflow_onomem`.
    pub pflow_onomem: u64,
    /// `pflow_oerrors`.
    pub pflow_oerrors: u64,
}

// Supported flow protocols
/// `PFLOW_PROTO_5`: original pflow.
pub const PFLOW_PROTO_5: u8 = 5;
/// `PFLOW_PROTO_10`: ipfix.
pub const PFLOW_PROTO_10: u8 = 10;
/// `PFLOW_PROTO_MAX`.
pub const PFLOW_PROTO_MAX: u8 = 11;

/// `PFLOW_PROTO_DEFAULT`.
pub const PFLOW_PROTO_DEFAULT: u8 = PFLOW_PROTO_5;

/// `struct pflow_protos`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PflowProtos {
    /// `ppr_name`.
    pub ppr_name: &'static str,
    /// `ppr_proto`.
    pub ppr_proto: u8,
}

/// `PFLOW_PROTOS`.
pub const PFLOW_PROTOS: [PflowProtos; 2] = [
    PflowProtos {
        ppr_name: "5",
        ppr_proto: PFLOW_PROTO_5,
    },
    PflowProtos {
        ppr_name: "10",
        ppr_proto: PFLOW_PROTO_10,
    },
];

/// `struct pflowreq`: the configuration `SIOCSETPFLOW` and `SIOCGETPFLOW` carry.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Pflowreq {
    /// `flowsrc`.
    pub flowsrc: SockaddrStorage,
    /// `flowdst`.
    pub flowdst: SockaddrStorage,
    /// `addrmask`: `PFLOW_MASK_*`.
    pub addrmask: u16,
    /// `version`.
    pub version: u8,
    /// The tail padding the C compiler adds (to the 8-byte alignment).
    pub _pad: [u8; 5],
}

// SAFETY: `#[repr(C)]` byte arrays and integers, the tail padding named: 520 bytes without
// implicit padding (checked below), any bit pattern valid.
unsafe impl AbiPod for Pflowreq {}

/// `PFLOW_MASK_SRCIP`.
pub const PFLOW_MASK_SRCIP: u16 = 0x01;
/// `PFLOW_MASK_DSTIP`.
pub const PFLOW_MASK_DSTIP: u16 = 0x02;
/// `PFLOW_MASK_VERSION`.
pub const PFLOW_MASK_VERSION: u16 = 0x04;

/// `struct pflow_softc`. Locks: \[I\] immutable after creation, \[m\] `sc_mtx`, \[p\]
/// `sc_lock`.
pub struct PflowSoftc {
    /// `sc_mtx`.
    pub sc_mtx: Mutex,
    /// `sc_lock`.
    pub sc_lock: Rwlock,

    /// `sc_dying`: \[p\].
    pub sc_dying: Cell<i32>,
    /// `sc_if`.
    pub sc_if: Ifnet,

    /// `sc_count`: \[m\].
    pub sc_count: Cell<u32>,
    /// `sc_count4`: \[m\].
    pub sc_count4: Cell<u32>,
    /// `sc_count4_nat`: \[m\].
    pub sc_count4_nat: Cell<u32>,
    /// `sc_count6`: \[m\].
    pub sc_count6: Cell<u32>,
    /// `sc_maxcount`: \[m\].
    pub sc_maxcount: Cell<u32>,
    /// `sc_maxcount4`: \[m\].
    pub sc_maxcount4: Cell<u32>,
    /// `sc_maxcount6`: \[m\].
    pub sc_maxcount6: Cell<u32>,
    /// `sc_gcounter`: \[m\].
    pub sc_gcounter: Cell<u32>,
    /// `sc_sequence`: \[m\].
    pub sc_sequence: Cell<u32>,
    /// `sc_tmo`.
    pub sc_tmo: Timeout,
    /// `sc_tmo6`.
    pub sc_tmo6: Timeout,
    /// `sc_tmo_tmpl`.
    pub sc_tmo_tmpl: Timeout,
    /// `sc_tmo_nat`.
    pub sc_tmo_nat: Timeout,
    /// `sc_outputqueue`.
    pub sc_outputqueue: MbufQueue,
    /// `sc_outputtask`.
    pub sc_outputtask: Task,
    /// `so`: \[p\] the UDP socket.
    pub so: Cell<Option<&'static Socket>>,
    /// `send_nam`: \[p\] the collector's address, as `sosend` takes it.
    pub send_nam: Cell<Option<&'static Mbuf>>,
    /// `sc_flowsrc`: \[p\].
    pub sc_flowsrc: Cell<Option<SockaddrStorage>>,
    /// `sc_flowdst`: \[p\].
    pub sc_flowdst: Cell<Option<SockaddrStorage>>,
    /// `sc_tmpl_ipfix`: \[I\].
    pub sc_tmpl_ipfix: Cell<PflowIpfixTmpl>,
    /// `sc_version`: \[m\].
    pub sc_version: Cell<u8>,
    /// `sc_mbuf`: \[m\] current cumulative mbuf.
    pub sc_mbuf: Cell<Option<&'static Mbuf>>,
    /// `sc_mbuf6`: \[m\] current cumulative mbuf.
    pub sc_mbuf6: Cell<Option<&'static Mbuf>>,
    /// `sc_mbuf_nat`: \[m\] current cumulative mbuf.
    pub sc_mbuf_nat: Cell<Option<&'static Mbuf>>,
    /// `sc_next`.
    pub sc_next: SlistEntry<PflowSoftc>,
}

// SAFETY: the members change under the locks their docs name, as in C; the interface as
// `Ifnet` documents.
unsafe impl Sync for PflowSoftc {}

impl PflowSoftc {
    /// The softc behind a `void *` argument (`timeout(9)`, `task_add(9)`).
    fn of_arg(arg: *mut c_void) -> &'static Self {
        // SAFETY: every timeout and task that names the softc as its argument is set up by
        // `pflow_clone_create` and stopped before `pflow_clone_destroy` frees it.
        unsafe { &*arg.cast::<Self>().cast_const() }
    }

    /// The softc of a `pflow` interface.
    fn of_ifp(ifp: &Ifnet) -> &'static Self {
        // SAFETY: `if_softc` of a `pflow` interface is its softc (`pflow_clone_create`),
        // which lives until `pflow_clone_destroy`.
        unsafe { &*ifp.if_softc.get().cast::<Self>().cast_const() }
    }

    /// The softc as a timeout or task argument.
    fn arg(&self) -> *mut c_void {
        ptr::from_ref(self).cast_mut().cast()
    }
}

crate::queue_adapter!(
    /// `SMR_SLIST_HEAD(, pflow_softc)` through `sc_next`.
    pub PflowifList: PflowSoftc, sc_next => SlistEntry<PflowSoftc>
);

/// `PFLOW_MINMTU`.
const PFLOW_MINMTU: usize = size_of::<PflowHeader>() + size_of::<PflowFlow>();

/// `enum pflowstat_counters`.
#[repr(usize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PflowstatCounters {
    /// `pflow_flows`.
    PflowFlows,
    /// `pflow_packets`.
    PflowPackets,
    /// `pflow_onomem`.
    PflowOnomem,
    /// `pflow_oerrors`.
    PflowOerrors,
    /// `pflow_ncounters`.
    PflowNcounters,
}

/// `pflow_ncounters`.
pub const PFLOW_NCOUNTERS: usize = PflowstatCounters::PflowNcounters as usize;

/// `pflowif_list`: the `pflow` interfaces, changed under the kernel lock.
pub static PFLOWIF_LIST: PfGlobal<SlistHead<PflowifList>> = PfGlobal(SlistHead::new());

/// `pflow_counters`.
pub static PFLOW_COUNTERS: [AtomicU64; PFLOW_NCOUNTERS] =
    [const { AtomicU64::new(0) }; PFLOW_NCOUNTERS];

/// `pflow_cloner`.
pub static PFLOW_CLONER: IfClone =
    IfClone::new(b"pflow", pflow_clone_create, Some(pflow_clone_destroy));

/// The bytes of a wire structure.
fn wire_bytes<T: AbiPod>(v: &T) -> &[u8] {
    // SAFETY: `T: AbiPod` has no padding, so its `size_of::<T>()` bytes are initialised and
    // may be read as `u8`s while `v` is borrowed.
    unsafe { slice::from_raw_parts(ptr::from_ref(v).cast::<u8>(), size_of::<T>()) }
}

/// Reads a wire structure from the start of `buf`.
fn wire_get<T: AbiPod>(buf: &[u8]) -> T {
    let b = &buf[..size_of::<T>()];
    // SAFETY: `b` holds `size_of::<T>()` bytes and every bit pattern is a valid `T`
    // (`AbiPod`); the read does not need alignment.
    unsafe { ptr::read_unaligned(b.as_ptr().cast::<T>()) }
}

/// The wire structure at offset `off` of the packet `m`.
fn m_get_wire<T: AbiPod>(m: &Mbuf, off: i32) -> T {
    let mut b = [0u8; 64];
    let b = &mut b[..size_of::<T>()];
    m_copydata(m, off, b);
    wire_get(b)
}

/// `pflowstat_inc`.
fn pflowstat_inc(c: PflowstatCounters) {
    PFLOW_COUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `pflowattach`: the pseudo-device attach function: registers the cloner.
pub fn pflowattach(_npflow: i32) {
    // SMR_SLIST_INIT(&pflowif_list): the static is empty.
    // pflow_counters = counters_alloc(pflow_ncounters): a static (see the deviations).

    // SAFETY: `pflowattach` runs once, from `main`'s pseudo-device attach.
    unsafe { if_clone_attach(&PFLOW_CLONER) };
}

/// `pflow_output`: drops the packet.
///
/// # Safety
///
/// As for `if_output` (`IfOutputFn`).
pub unsafe fn pflow_output(
    _ifp: &'static Ifnet,
    m: &'static Mbuf,
    _dst: *const Sockaddr,
    _rt: Option<&'static Rtentry>,
) -> Result<(), Errno> {
    m_freem(m); // drop packet
    Err(Errno::EAFNOSUPPORT)
}

/// `pflow_output_process`: `sc_outputtask`: sends the queued datagrams.
fn pflow_output_process(arg: *mut c_void) {
    let sc = PflowSoftc::of_arg(arg);
    let ml = MbufList::new();

    mq_delist(&sc.sc_outputqueue, &ml);
    rw_enter_read(&sc.sc_lock);
    while let Some(m) = ml_dequeue(&ml) {
        let _ = pflow_sendout_mbuf(sc, m);
    }
    rw_exit_read(&sc.sc_lock);
}

/// A template field: `id` of `len` bytes, in network order.
const fn fspec(id: u16, len: u16) -> PflowTmplFspec {
    PflowTmplFspec {
        field_id: htons(id),
        len: htons(len),
    }
}

/// The IPFIX templates `pflow_clone_create` fills in.
const fn pflow_ipfix_tmpl() -> PflowIpfixTmpl {
    PflowIpfixTmpl {
        set_header: PflowSetHeader {
            set_id: htons(PFLOW_IPFIX_TMPL_SET_ID),
            set_length: htons(size_of::<PflowIpfixTmpl>() as u16),
        },

        // ipfix IPv4 template
        ipv4_tmpl: PflowIpfixTmplIpv4 {
            h: PflowTmplHdr {
                tmpl_id: htons(PFLOW_IPFIX_TMPL_IPV4_ID),
                field_count: htons(PFLOW_IPFIX_TMPL_IPV4_FIELD_COUNT),
            },
            src_ip: fspec(PFIX_IE_sourceIPv4Address, 4),
            dest_ip: fspec(PFIX_IE_destinationIPv4Address, 4),
            if_index_in: fspec(PFIX_IE_ingressInterface, 4),
            if_index_out: fspec(PFIX_IE_egressInterface, 4),
            packets: fspec(PFIX_IE_packetDeltaCount, 8),
            octets: fspec(PFIX_IE_octetDeltaCount, 8),
            start: fspec(PFIX_IE_flowStartMilliseconds, 8),
            finish: fspec(PFIX_IE_flowEndMilliseconds, 8),
            src_port: fspec(PFIX_IE_sourceTransportPort, 2),
            dest_port: fspec(PFIX_IE_destinationTransportPort, 2),
            tos: fspec(PFIX_IE_ipClassOfService, 1),
            protocol: fspec(PFIX_IE_protocolIdentifier, 1),
        },

        // ipfix IPv4 NAT template
        ipv4_nat_tmpl: PflowIpfixTmplNatIpv4 {
            h: PflowTmplHdr {
                tmpl_id: htons(PFLOW_IPFIX_TMPL_NAT_IPV4_ID),
                field_count: htons(PFLOW_IPFIX_TMPL_NAT_IPV4_FIELD_COUNT),
            },
            src_ip: fspec(PFIX_IE_sourceIPv4Address, 4),
            dest_ip: fspec(PFIX_IE_destinationIPv4Address, 4),
            if_index_in: fspec(PFIX_IE_ingressInterface, 4),
            if_index_out: fspec(PFIX_IE_egressInterface, 4),
            packets: fspec(PFIX_IE_packetDeltaCount, 8),
            octets: fspec(PFIX_IE_octetDeltaCount, 8),
            start: fspec(PFIX_IE_flowStartMilliseconds, 8),
            finish: fspec(PFIX_IE_flowEndMilliseconds, 8),
            post_src_ip: fspec(PFIX_IE_postNATSourceIPv4Address, 4),
            post_dest_ip: fspec(PFIX_IE_postNATDestinationIPv4Address, 4),
            post_src_port: fspec(PFIX_IE_postNAPTSourceTransportPort, 2),
            post_dest_port: fspec(PFIX_IE_postNAPTDestinationTransportPort, 2),
            src_port: fspec(PFIX_IE_sourceTransportPort, 2),
            dest_port: fspec(PFIX_IE_destinationTransportPort, 2),
            tos: fspec(PFIX_IE_ipClassOfService, 1),
            protocol: fspec(PFIX_IE_protocolIdentifier, 1),
        },

        // ipfix IPv6 template
        ipv6_tmpl: PflowIpfixTmplIpv6 {
            h: PflowTmplHdr {
                tmpl_id: htons(PFLOW_IPFIX_TMPL_IPV6_ID),
                field_count: htons(PFLOW_IPFIX_TMPL_IPV6_FIELD_COUNT),
            },
            src_ip: fspec(PFIX_IE_sourceIPv6Address, 16),
            dest_ip: fspec(PFIX_IE_destinationIPv6Address, 16),
            if_index_in: fspec(PFIX_IE_ingressInterface, 4),
            if_index_out: fspec(PFIX_IE_egressInterface, 4),
            packets: fspec(PFIX_IE_packetDeltaCount, 8),
            octets: fspec(PFIX_IE_octetDeltaCount, 8),
            start: fspec(PFIX_IE_flowStartMilliseconds, 8),
            finish: fspec(PFIX_IE_flowEndMilliseconds, 8),
            src_port: fspec(PFIX_IE_sourceTransportPort, 2),
            dest_port: fspec(PFIX_IE_destinationTransportPort, 2),
            tos: fspec(PFIX_IE_ipClassOfService, 1),
            protocol: fspec(PFIX_IE_protocolIdentifier, 1),
        },
    }
}

/// `pflow_clone_create`: creates `pflow<unit>`.
pub fn pflow_clone_create(_ifc: &'static IfClone, unit: i32) -> Result<(), Errno> {
    let Some(mem) = malloc(size_of::<PflowSoftc>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
        panic(format_args!("pflow_clone_create: out of memory"));
    };
    // SAFETY: a zero-filled block of `sizeof(struct pflow_softc)` bytes, aligned for it
    // (malloc's chunks are aligned to their size); the all-zero softc is valid (see the
    // module's docs). It lives until `pflow_clone_destroy`.
    let pflowif: &'static PflowSoftc = unsafe { &*mem.as_ptr().cast::<PflowSoftc>() };
    rw_init(&pflowif.sc_lock, "pflowlk");
    mtx_init(&pflowif.sc_mtx, IPL_MPFLOOR);
    let Some(nam) = m_get(M_WAIT, MT_SONAME) else {
        panic(format_args!("pflow_clone_create: no mbuf"));
    };
    pflowif.send_nam.set(Some(nam));
    pflowif.sc_version.set(PFLOW_PROTO_DEFAULT);

    // ipfix template init
    pflowif.sc_tmpl_ipfix.set(pflow_ipfix_tmpl());

    let ifp = &pflowif.sc_if;
    let mut xname = [0u8; IFNAMSIZ];
    let _ = snprintf(&mut xname, format_args!("pflow{unit}"));
    ifp.if_xname.set(xname);
    ifp.if_softc.set(pflowif.arg());
    ifp.if_ioctl.set(Some(pflowioctl));
    ifp.if_output.set(Some(pflow_output));
    ifp.if_start.set(None);
    ifp.if_xflags.set(IFXF_CLONED);
    ifp.if_type.set(IFT_PFLOW);
    ifp.if_hdrlen.set(PFLOW_HDRLEN as u8);
    ifp.if_flags.set(IFF_UP);
    ifp.if_flags.set(ifp.if_flags.get() & !IFF_RUNNING); // not running, need receiver
    mq_init(&pflowif.sc_outputqueue, 8192, IPL_SOFTNET);
    pflow_setmtu(pflowif, ETHERMTU as i32);

    timeout_set_proc(&pflowif.sc_tmo, pflow_timeout, pflowif.arg());
    timeout_set_proc(&pflowif.sc_tmo6, pflow_timeout6, pflowif.arg());
    timeout_set_proc(&pflowif.sc_tmo_tmpl, pflow_timeout_tmpl, pflowif.arg());
    timeout_set_proc(&pflowif.sc_tmo_nat, pflow_timeout_nat, pflowif.arg());

    task_set(&pflowif.sc_outputtask, pflow_output_process, pflowif.arg());

    if_counters_alloc(ifp);
    if_attach(ifp);
    if_alloc_sadl(ifp);

    // Insert into list of pflows
    // KERNEL_ASSERT_LOCKED(): one CPU.
    // SAFETY: the new softc is on no list; it stays allocated until `pflow_clone_destroy`
    // takes it off.
    unsafe { PFLOWIF_LIST.insert_head(pflowif) };
    Ok(())
}

/// `pflow_clone_destroy`.
pub fn pflow_clone_destroy(ifp: &'static Ifnet) -> Result<(), Errno> {
    let sc = PflowSoftc::of_ifp(ifp);
    let mut error = Ok(());

    rw_enter_write(&sc.sc_lock);
    sc.sc_dying.set(1);
    rw_exit_write(&sc.sc_lock);

    // KERNEL_ASSERT_LOCKED(): one CPU.
    // SAFETY: `pflow_clone_create` put the softc on the list.
    unsafe { PFLOWIF_LIST.remove(sc) };
    // smr_barrier(): no SMR (see the module's deviations).

    let _ = timeout_del(&sc.sc_tmo);
    let _ = timeout_del(&sc.sc_tmo6);
    let _ = timeout_del(&sc.sc_tmo_tmpl);
    let _ = timeout_del(&sc.sc_tmo_nat);

    pflow_flush(sc);
    if let Some(tq) = net_tq(ifp.if_index.get()) {
        taskq_del_barrier(tq, &sc.sc_outputtask);
    }
    let _ = mq_purge(&sc.sc_outputqueue);
    m_freem(sc.send_nam.take());
    if let Some(so) = sc.so.take() {
        error = soclose(so, MSG_DONTWAIT);
    }
    sc.sc_flowdst.set(None);
    sc.sc_flowsrc.set(None);
    if_detach(ifp);
    free(NonNull::from(sc).cast(), M_DEVBUF, size_of::<PflowSoftc>());
    error
}

/// The bytes of a socket address.
fn ss_bytes(ss: &SockaddrStorage) -> &[u8] {
    // SAFETY: `SockaddrStorage` is `#[repr(C)]` integers and byte arrays without padding (256
    // bytes); viewing it as bytes while borrowed is sound.
    unsafe { slice::from_raw_parts(ptr::from_ref(ss).cast::<u8>(), size_of::<SockaddrStorage>()) }
}

/// A `struct sockaddr_in` read from a socket address.
fn ss_sin(ss: &SockaddrStorage) -> SockaddrIn {
    let b = ss_bytes(ss);
    // SAFETY: the storage holds more than `size_of::<SockaddrIn>()` bytes; every bit pattern
    // of the plain-integer `SockaddrIn` is valid and the read does not need alignment.
    unsafe { ptr::read_unaligned(b.as_ptr().cast::<SockaddrIn>()) }
}

/// `pflowvalidsockaddr`: whether `sa` is an address to bind to (`ignore_port`) or send to.
pub fn pflowvalidsockaddr(sa: Option<&SockaddrStorage>, ignore_port: bool) -> bool {
    let Some(sa) = sa else {
        return false;
    };
    match sa.ss_family {
        AF_INET => {
            let sin = ss_sin(sa);
            sin.sin_addr.s_addr != INADDR_ANY && (ignore_port || sin.sin_port != 0)
        }
        AF_INET6 => {
            let b = ss_bytes(sa);
            // SAFETY: the storage holds more than `size_of::<SockaddrIn6>()` bytes; every bit
            // pattern of the plain-integer `SockaddrIn6` is valid and the read is unaligned.
            let sin6 = unsafe { ptr::read_unaligned(b.as_ptr().cast::<SockaddrIn6>()) };
            !in6_is_addr_unspecified(&sin6.sin6_addr) && (ignore_port || sin6.sin6_port != 0)
        }
        _ => false,
    }
}

/// The copy of `ss` the C `malloc`s for a family it knows, its `sa_len` set to the family's
/// size; `None` for another family.
fn pflow_sockaddr_copy(ss: &SockaddrStorage) -> Option<SockaddrStorage> {
    let len = match ss.ss_family {
        AF_INET => size_of::<SockaddrIn>() as u8,
        AF_INET6 => size_of::<SockaddrIn6>() as u8,
        _ => return None,
    };
    // memcpy(sc->sc_flow*, &pflowr->flow*, sizeof(struct sockaddr_in*)): the rest of the
    // storage is not part of the address.
    let n = usize::from(len);
    let mut b = [0u8; size_of::<SockaddrStorage>()];
    b[..n].copy_from_slice(&ss_bytes(ss)[..n]);
    // SAFETY: `b` holds the bytes of a storage, of its size; every bit pattern of the
    // plain-integer `SockaddrStorage` is valid and the read does not need alignment.
    let mut copy: SockaddrStorage = unsafe { ptr::read_unaligned(b.as_ptr().cast()) };
    copy.ss_len = len;
    Some(copy)
}

/// An `MT_SONAME` mbuf holding the first `sa_len` bytes of `ss` (`sobind`'s and `sosend`'s
/// address).
fn pflow_nam_fill(m: &Mbuf, ss: &SockaddrStorage) {
    let len = usize::from(ss.ss_len).min(size_of::<SockaddrStorage>());
    m.m_len().set(len as u32);
    let _ = m_copyback(m, 0, &ss_bytes(ss)[..len], M_NOWAIT);
}

/// `pflow_set`: applies a `SIOCSETPFLOW` request: addresses, socket and version. Called with
/// `sc_lock` held for writing.
pub fn pflow_set(sc: &PflowSoftc, pflowr: &Pflowreq) -> Result<(), Errno> {
    if pflowr.addrmask & PFLOW_MASK_VERSION != 0 {
        match pflowr.version {
            PFLOW_PROTO_5 | PFLOW_PROTO_10 => {}
            _ => return Err(Errno::EINVAL),
        }
    }

    rw_assert_wrlock(&sc.sc_lock);

    pflow_flush(sc);

    if pflowr.addrmask & PFLOW_MASK_DSTIP != 0 {
        if let Some(dst) = sc.sc_flowdst.get()
            && dst.ss_family != pflowr.flowdst.ss_family
        {
            sc.sc_flowdst.set(None);
            if let Some(so) = sc.so.take() {
                let _ = soclose(so, MSG_DONTWAIT);
            }
        }

        if let Some(copy) = pflow_sockaddr_copy(&pflowr.flowdst) {
            sc.sc_flowdst.set(Some(copy));
        }

        if let Some(dst) = sc.sc_flowdst.get()
            && let Some(nam) = sc.send_nam.get()
        {
            pflow_nam_fill(nam, &dst);
        }
    }

    if pflowr.addrmask & PFLOW_MASK_SRCIP != 0 {
        sc.sc_flowsrc.set(None);
        if let Some(so) = sc.so.take() {
            let _ = soclose(so, MSG_DONTWAIT);
        }
        sc.sc_flowsrc.set(pflow_sockaddr_copy(&pflowr.flowsrc));
    }

    if sc.so.get().is_none() {
        let dst = sc.sc_flowdst.get();
        if pflowvalidsockaddr(dst.as_ref(), false)
            && let Some(dst) = dst
        {
            let so = socreate(i32::from(dst.ss_family), SOCK_DGRAM, 0)?;
            let src = sc.sc_flowsrc.get();
            if pflowvalidsockaddr(src.as_ref(), true)
                && let Some(src) = src
            {
                let Some(m) = m_get(M_WAIT, MT_SONAME) else {
                    let _ = soclose(so, MSG_DONTWAIT);
                    return Err(Errno::ENOBUFS);
                };
                pflow_nam_fill(m, &src);

                let Some(p) = curproc() else {
                    panic(format_args!("pflow_set: no curproc"));
                };
                solock(so);
                let error = sobind(so, m, p);
                sounlock(so);
                m_freem(m);
                if let Err(e) = error {
                    let _ = soclose(so, MSG_DONTWAIT);
                    return Err(e);
                }
            }
            sc.so.set(Some(so));
        }
    } else if !pflowvalidsockaddr(sc.sc_flowdst.get().as_ref(), false)
        && let Some(so) = sc.so.take()
    {
        let _ = soclose(so, MSG_DONTWAIT);
    }

    net_lock();
    mtx_enter(&sc.sc_mtx);

    // error check is above
    if pflowr.addrmask & PFLOW_MASK_VERSION != 0 {
        sc.sc_version.set(pflowr.version);
    }

    pflow_setmtu(sc, ETHERMTU as i32);

    match sc.sc_version.get() {
        PFLOW_PROTO_5 => {
            let _ = timeout_del(&sc.sc_tmo6);
            let _ = timeout_del(&sc.sc_tmo_tmpl);
        }
        PFLOW_PROTO_10 => {
            let _ = timeout_add_sec(&sc.sc_tmo_tmpl, PFLOW_TMPL_TIMEOUT);
        }
        _ => {} // NOTREACHED
    }

    mtx_leave(&sc.sc_mtx);
    net_unlock();

    Ok(())
}

/// Sets `IFF_RUNNING` when the interface is up and has a socket, and sends the IPFIX
/// templates then (the tail of `SIOCSIFFLAGS` and `SIOCSETPFLOW`). Called with the net lock.
fn pflow_set_running(ifp: &Ifnet, sc: &PflowSoftc) {
    if ifp.if_flags.get() & IFF_UP != 0 && sc.so.get().is_some() {
        ifp.if_flags.set(ifp.if_flags.get() | IFF_RUNNING);
        mtx_enter(&sc.sc_mtx);
        // send templates on startup
        if sc.sc_version.get() == PFLOW_PROTO_10 {
            let _ = pflow_sendout_ipfix_tmpl(sc);
        }
        mtx_leave(&sc.sc_mtx);
    } else {
        ifp.if_flags.set(ifp.if_flags.get() & !IFF_RUNNING);
    }
}

/// `pflowioctl`.
///
/// # Safety
///
/// As for `if_ioctl` (`IfIoctlFn`): `data` is the command's `struct ifreq`, aligned; the
/// caller holds the net lock.
pub unsafe fn pflowioctl(ifp: &'static Ifnet, cmd: u64, data: *mut u8) -> Result<(), Errno> {
    let sc = PflowSoftc::of_ifp(ifp);

    match cmd {
        SIOCSIFADDR | SIOCSIFDSTADDR | SIOCSIFFLAGS | SIOCSIFMTU | SIOCGETPFLOW | SIOCSETPFLOW => {}
        _ => return Err(Errno::ENOTTY),
    }

    // SAFETY: the caller's contract: every command handled here takes a `struct ifreq`.
    let ifr = unsafe { &mut *data.cast::<Ifreq>() };

    // XXXSMP: enforce lock order
    net_unlock();
    rw_enter_write(&sc.sc_lock);

    let error = 'out: {
        if sc.sc_dying.get() != 0 {
            break 'out Err(Errno::ENXIO);
        }

        match cmd {
            SIOCSIFADDR | SIOCSIFDSTADDR | SIOCSIFFLAGS => {
                net_lock();
                pflow_set_running(ifp, sc);
                net_unlock();
            }

            SIOCSIFMTU => {
                if (ifr.ifr_mtu() as i64) < PFLOW_MINMTU as i64 {
                    break 'out Err(Errno::EINVAL);
                }
                if ifr.ifr_mtu() > MCLBYTES as i32 {
                    ifr.set_ifr_mtu(MCLBYTES as i32);
                }
                net_lock();
                if (ifr.ifr_mtu() as u32) < ifp.if_mtu.get() {
                    pflow_flush(sc);
                }
                mtx_enter(&sc.sc_mtx);
                pflow_setmtu(sc, ifr.ifr_mtu());
                mtx_leave(&sc.sc_mtx);
                net_unlock();
            }

            SIOCGETPFLOW => {
                let mut pflowr = Pflowreq::default();

                if let Some(src) = sc.sc_flowsrc.get() {
                    pflowr.flowsrc = src;
                }
                if let Some(dst) = sc.sc_flowdst.get() {
                    pflowr.flowdst = dst;
                }
                mtx_enter(&sc.sc_mtx);
                pflowr.version = sc.sc_version.get();
                mtx_leave(&sc.sc_mtx);

                if let Err(e) = copyout_obj(&pflowr, ifr.ifr_data() as usize) {
                    break 'out Err(e);
                }
            }

            SIOCSETPFLOW => {
                let Some(p) = curproc() else {
                    break 'out Err(Errno::EPERM);
                };
                if let Err(e) = suser(p) {
                    break 'out Err(e);
                }
                let pflowr: Pflowreq = match copyin_obj(ifr.ifr_data() as usize) {
                    Ok(r) => r,
                    Err(e) => break 'out Err(e),
                };

                if let Err(e) = pflow_set(sc, &pflowr) {
                    break 'out Err(e);
                }

                net_lock();
                pflow_set_running(ifp, sc);
                net_unlock();
            }

            _ => {}
        }
        Ok(())
    };

    // out:
    rw_exit_write(&sc.sc_lock);
    net_lock();

    error
}

/// `pflow_calc_mtu`: the IPFIX interface MTU for `mtu` and the header size `hdrsz`; sets the
/// per-family record limits.
pub fn pflow_calc_mtu(sc: &PflowSoftc, mtu: i32, hdrsz: usize) -> i32 {
    let room = (mtu as usize)
        .wrapping_sub(hdrsz)
        .wrapping_sub(size_of::<Udpiphdr>());
    let max4 = (room / size_of::<PflowIpfixNatFlow4>()).min(PFLOW_MAXFLOWS as usize);
    let max6 = (room / size_of::<PflowIpfixFlow6>()).min(PFLOW_MAXFLOWS as usize);
    sc.sc_maxcount4.set(max4 as u32);
    sc.sc_maxcount6.set(max6 as u32);
    (hdrsz
        + size_of::<Udpiphdr>()
        + (max4 * size_of::<PflowIpfixNatFlow4>()).min(max6 * size_of::<PflowIpfixFlow6>()))
        as i32
}

/// `pflow_setmtu`: the interface MTU for the requested `mtu_req`, a whole number of records.
pub fn pflow_setmtu(sc: &PflowSoftc, mtu_req: i32) {
    let mtu = mtu_req;

    match sc.sc_version.get() {
        PFLOW_PROTO_5 => {
            let room = (mtu as usize)
                .wrapping_sub(size_of::<PflowHeader>())
                .wrapping_sub(size_of::<Udpiphdr>());
            let max = (room / size_of::<PflowFlow>()).min(PFLOW_MAXFLOWS as usize);
            sc.sc_maxcount.set(max as u32);
            sc.sc_if.if_mtu.set(
                (size_of::<PflowHeader>() + size_of::<Udpiphdr>() + max * size_of::<PflowFlow>())
                    as u32,
            );
        }
        PFLOW_PROTO_10 => {
            sc.sc_if
                .if_mtu
                .set(pflow_calc_mtu(sc, mtu, size_of::<PflowV10Header>()) as u32);
        }
        _ => {} // NOTREACHED
    }
}

/// `pflow_get_mbuf`: an empty cluster mbuf for a new datagram, its header (version 5) or set
/// header (`set_id`, IPFIX) in place. Called with `sc_mtx` held.
pub fn pflow_get_mbuf(sc: &PflowSoftc, set_id: u16) -> Option<&'static Mbuf> {
    mutex_assert_locked(&sc.sc_mtx, "pflow_get_mbuf");

    let Some(m) = m_gethdr(M_DONTWAIT, MT_DATA) else {
        pflowstat_inc(PflowstatCounters::PflowOnomem);
        return None;
    };

    mclget(m, M_DONTWAIT);
    if m.m_flags().get() & M_EXT == 0 {
        m_free(m);
        pflowstat_inc(PflowstatCounters::PflowOnomem);
        return None;
    }

    m.m_len().set(0);
    m.m_pkthdr().len.set(0);
    m.m_pkthdr().ph_ifidx.set(0);

    // sc == NULL (get only a new empty mbuf): no caller (see the deviations).

    match sc.sc_version.get() {
        PFLOW_PROTO_5 => {
            // populate pflow_header
            let h = PflowHeader {
                reserved1: 0,
                reserved2: 0,
                count: 0,
                version: htons(u16::from(PFLOW_PROTO_5)),
                flow_sequence: htonl(sc.sc_gcounter.get()),
                engine_type: PFLOW_ENGINE_TYPE,
                engine_id: PFLOW_ENGINE_ID,
                ..PflowHeader::default()
            };
            let _ = m_copyback(m, 0, wire_bytes(&h), M_NOWAIT);

            sc.sc_count.set(0);
            let _ = timeout_add_sec(&sc.sc_tmo, PFLOW_TIMEOUT);
        }
        PFLOW_PROTO_10 => {
            // populate pflow_set_header
            let set_hdr = PflowSetHeader {
                set_length: 0,
                set_id: htons(set_id),
            };
            let _ = m_copyback(m, 0, wire_bytes(&set_hdr), M_NOWAIT);
        }
        _ => {} // NOTREACHED
    }

    Some(m)
}

/// `sk->addr[i].v4.s_addr`.
fn sk_v4(sk: &PfStateKey, i: usize) -> u32 {
    sk.addr[i].get().addr32(0)
}

/// `copy_flow_data`: the two version 5 records of a state (`src` and `dst` are the key's
/// indices of the flow's source and destination), with the byte counters `bytes`.
pub fn copy_flow_data(
    flow1: &mut PflowFlow,
    flow2: &mut PflowFlow,
    st: &'static PfState,
    sk: &PfStateKey,
    bytes: [u64; 2],
    src: usize,
    dst: usize,
) {
    flow1.src_ip = sk_v4(sk, src);
    flow2.dest_ip = flow1.src_ip;
    flow1.src_port = sk.port[src].get();
    flow2.dest_port = flow1.src_port;
    flow1.dest_ip = sk_v4(sk, dst);
    flow2.src_ip = flow1.dest_ip;
    flow1.dest_port = sk.port[dst].get();
    flow2.src_port = flow1.dest_port;

    flow1.dest_as = 0;
    flow2.src_as = 0;
    flow1.src_as = 0;
    flow2.dest_as = 0;
    flow1.if_index_in = htons(st.if_index_in.get());
    flow1.if_index_out = htons(st.if_index_out.get());
    flow2.if_index_in = htons(st.if_index_out.get());
    flow2.if_index_out = htons(st.if_index_in.get());
    flow1.dest_mask = 0;
    flow2.src_mask = 0;
    flow1.src_mask = 0;
    flow2.dest_mask = 0;

    flow1.flow_packets = htonl(st.packets[0].get() as u32);
    flow2.flow_packets = htonl(st.packets[1].get() as u32);
    flow1.flow_octets = htonl(bytes[0] as u32);
    flow2.flow_octets = htonl(bytes[1] as u32);

    // Pretend the flow was created or expired when the machine came up when creation is in
    // the future of the last time a package was seen or was created / expired before this
    // machine came up due to pfsync.
    let (creation, expire) = (st.creation.get(), st.expire.get());
    flow1.flow_start = if creation < 0 || creation > expire {
        htonl(0)
    } else {
        htonl(creation.wrapping_mul(1000) as u32)
    };
    flow2.flow_start = flow1.flow_start;
    flow1.flow_finish = if expire < 0 {
        htonl(0)
    } else {
        htonl(expire.wrapping_mul(1000) as u32)
    };
    flow2.flow_finish = flow1.flow_finish;
    flow1.tcp_flags = 0;
    flow2.tcp_flags = 0;
    flow1.protocol = sk.proto.get();
    flow2.protocol = flow1.protocol;
    flow1.tos = st.rule.ptr().map_or(0, |r| r.tos);
    flow2.tos = flow1.tos;
}

/// The IPFIX start and finish of a state, in milliseconds since the epoch.
fn ipfix_times(st: &PfState) -> (i64, i64) {
    let (now, up) = (gettime(), getuptime());
    let (creation, expire) = (i64::from(st.creation.get()), i64::from(st.expire.get()));

    // Pretend the flow was created when the machine came up when creation is in the future
    // of the last time a package was seen due to pfsync.
    let start = if creation > expire {
        (now - up) * 1000
    } else {
        (now - (up - creation)) * 1000
    };
    let finish = (now - (up - expire)) * 1000;
    (htobe64(start as u64) as i64, htobe64(finish as u64) as i64)
}

/// `copy_flow_ipfix_4_data`.
pub fn copy_flow_ipfix_4_data(
    flow1: &mut PflowIpfixFlow4,
    flow2: &mut PflowIpfixFlow4,
    st: &'static PfState,
    sk: &PfStateKey,
    _sc: &PflowSoftc,
    src: usize,
    dst: usize,
) {
    flow1.src_ip = sk_v4(sk, src);
    flow2.dest_ip = flow1.src_ip;
    flow1.src_port = sk.port[src].get();
    flow2.dest_port = flow1.src_port;
    flow1.dest_ip = sk_v4(sk, dst);
    flow2.src_ip = flow1.dest_ip;
    flow1.dest_port = sk.port[dst].get();
    flow2.src_port = flow1.dest_port;

    flow1.if_index_in = htonl(u32::from(st.if_index_in.get()));
    flow1.if_index_out = htonl(u32::from(st.if_index_out.get()));
    flow2.if_index_in = htonl(u32::from(st.if_index_out.get()));
    flow2.if_index_out = htonl(u32::from(st.if_index_in.get()));

    flow1.flow_packets = htobe64(st.packets[0].get());
    flow2.flow_packets = htobe64(st.packets[1].get());
    flow1.flow_octets = htobe64(st.bytes[0].get());
    flow2.flow_octets = htobe64(st.bytes[1].get());

    let (start, finish) = ipfix_times(st);
    flow1.flow_start = start;
    flow2.flow_start = start;
    flow1.flow_finish = finish;
    flow2.flow_finish = finish;

    flow1.protocol = sk.proto.get();
    flow2.protocol = flow1.protocol;
    flow1.tos = st.rule.ptr().map_or(0, |r| r.tos);
    flow2.tos = flow1.tos;
}

/// `copy_flow_ipfix_nat_4_data`.
#[allow(clippy::too_many_arguments)] // the C's
pub fn copy_flow_ipfix_nat_4_data(
    flow1: &mut PflowIpfixNatFlow4,
    flow2: &mut PflowIpfixNatFlow4,
    st: &'static PfState,
    sk: &PfStateKey,
    skw: &PfStateKey,
    _sc: &PflowSoftc,
    src: usize,
    dst: usize,
) {
    flow1.src_ip = sk_v4(sk, src);
    flow1.dest_ip = sk_v4(sk, dst);
    flow2.src_ip = sk_v4(sk, dst);
    flow2.dest_ip = sk_v4(sk, src);

    flow1.post_src_ip = sk_v4(skw, src);
    flow1.post_dest_ip = sk_v4(skw, dst);
    flow1.post_src_port = skw.port[src].get();
    flow1.post_dest_port = skw.port[dst].get();

    flow2.post_src_ip = sk_v4(skw, dst);
    flow2.post_dest_ip = sk_v4(skw, src);
    flow2.post_src_port = skw.port[dst].get();
    flow2.post_dest_port = skw.port[src].get();

    flow1.if_index_in = htonl(u32::from(st.if_index_in.get()));
    flow1.if_index_out = htonl(u32::from(st.if_index_out.get()));
    flow2.if_index_in = htonl(u32::from(st.if_index_out.get()));
    flow2.if_index_out = htonl(u32::from(st.if_index_in.get()));

    flow1.flow_packets = htobe64(st.packets[0].get());
    flow2.flow_packets = htobe64(st.packets[1].get());
    flow1.flow_octets = htobe64(st.bytes[0].get());
    flow2.flow_octets = htobe64(st.bytes[1].get());

    let (start, finish) = ipfix_times(st);
    flow1.flow_start = start;
    flow2.flow_start = start;
    flow1.flow_finish = finish;
    flow2.flow_finish = finish;

    flow1.src_port = sk.port[src].get();
    flow1.dest_port = sk.port[dst].get();
    flow2.src_port = sk.port[dst].get();
    flow2.dest_port = sk.port[src].get();

    flow1.protocol = sk.proto.get();
    flow2.protocol = flow1.protocol;
    flow1.tos = st.rule.ptr().map_or(0, |r| r.tos);
    flow2.tos = flow1.tos;
}

/// `copy_flow_ipfix_6_data`.
pub fn copy_flow_ipfix_6_data(
    flow1: &mut PflowIpfixFlow6,
    flow2: &mut PflowIpfixFlow6,
    st: &'static PfState,
    sk: &PfStateKey,
    _sc: &PflowSoftc,
    src: usize,
    dst: usize,
) {
    flow1.src_ip = sk.addr[src].get().v6();
    flow2.dest_ip = sk.addr[src].get().v6();
    flow1.src_port = sk.port[src].get();
    flow2.dest_port = flow1.src_port;
    flow1.dest_ip = sk.addr[dst].get().v6();
    flow2.src_ip = sk.addr[dst].get().v6();
    flow1.dest_port = sk.port[dst].get();
    flow2.src_port = flow1.dest_port;

    flow1.if_index_in = htonl(u32::from(st.if_index_in.get()));
    flow1.if_index_out = htonl(u32::from(st.if_index_out.get()));
    flow2.if_index_in = htonl(u32::from(st.if_index_out.get()));
    flow2.if_index_out = htonl(u32::from(st.if_index_in.get()));

    flow1.flow_packets = htobe64(st.packets[0].get());
    flow2.flow_packets = htobe64(st.packets[1].get());
    flow1.flow_octets = htobe64(st.bytes[0].get());
    flow2.flow_octets = htobe64(st.bytes[1].get());

    let (start, finish) = ipfix_times(st);
    flow1.flow_start = start;
    flow2.flow_start = start;
    flow1.flow_finish = finish;
    flow2.flow_finish = finish;

    flow1.protocol = sk.proto.get();
    flow2.protocol = flow1.protocol;
    flow1.tos = st.rule.ptr().map_or(0, |r| r.tos);
    flow2.tos = flow1.tos;
}

/// `export_pflow`: pf removes `st`, a state with `PFSTATE_PFLOW`: its flows go to every
/// `pflow` interface.
pub fn export_pflow(st: &'static PfState) -> i32 {
    let in_ = st.direction.get() == PF_IN;
    let out = st.direction.get() == PF_OUT;
    let sk = st.key[if in_ { PF_SK_WIRE } else { PF_SK_STACK }].get();
    let skw = st.key[if out { PF_SK_WIRE } else { PF_SK_STACK }].get();
    let (Some(sk), Some(skw)) = (sk, skw) else {
        return 0;
    };

    for sc in PFLOWIF_LIST.iter() {
        mtx_enter(&sc.sc_mtx);
        let af = sk.af.get();
        match sc.sc_version.get() {
            PFLOW_PROTO_5 if af == AF_INET => {
                let _ = export_pflow_if(st, sk, skw, sc);
            }
            PFLOW_PROTO_10 if af == AF_INET || af == AF_INET6 => {
                let _ = export_pflow_if(st, sk, skw, sc);
            }
            _ => {} // another family, or NOTREACHED
        }
        mtx_leave(&sc.sc_mtx);
    }

    0
}

/// `export_pflow_if`: the flows of `st` on one interface. A version 5 record counts at most
/// `PFLOW_MAXBYTES`: a bigger flow is split into several.
pub fn export_pflow_if(
    st: &'static PfState,
    sk: &PfStateKey,
    skw: &PfStateKey,
    sc: &PflowSoftc,
) -> Result<(), Errno> {
    let ifp = &sc.sc_if;

    if ifp.if_flags.get() & IFF_RUNNING == 0 {
        return Ok(());
    }

    if sc.sc_version.get() == PFLOW_PROTO_10 {
        return pflow_pack_flow_ipfix(st, sk, skw, sc);
    }

    // PFLOW_PROTO_5
    let stb = [st.bytes[0].get(), st.bytes[1].get()];
    if stb[0] < PFLOW_MAXBYTES && stb[1] < PFLOW_MAXBYTES {
        return pflow_pack_flow(st, sk, sc, stb);
    }

    // flow > PFLOW_MAXBYTES need special handling
    let mut bytes = stb;

    while bytes[0] > PFLOW_MAXBYTES {
        pflow_pack_flow(st, sk, sc, [PFLOW_MAXBYTES, 0])?;
        if bytes[0] - PFLOW_MAXBYTES > 0 {
            bytes[0] -= PFLOW_MAXBYTES;
        }
    }

    while bytes[1] > PFLOW_MAXBYTES {
        pflow_pack_flow(st, sk, sc, [0, PFLOW_MAXBYTES])?;
        if bytes[1] - PFLOW_MAXBYTES > 0 {
            bytes[1] -= PFLOW_MAXBYTES;
        }
    }

    pflow_pack_flow(st, sk, sc, bytes)
}

/// `copy_flow_to_m`: appends a version 5 record; a full datagram goes out. Called with
/// `sc_mtx` held.
pub fn copy_flow_to_m(flow: &PflowFlow, sc: &PflowSoftc) -> Result<(), Errno> {
    let mut ret = Ok(());

    mutex_assert_locked(&sc.sc_mtx, "copy_flow_to_m");

    if sc.sc_mbuf.get().is_none() {
        let Some(m) = pflow_get_mbuf(sc, 0) else {
            return Err(Errno::ENOBUFS);
        };
        sc.sc_mbuf.set(Some(m));
    }
    let off = PFLOW_HDRLEN + sc.sc_count.get() as usize * size_of::<PflowFlow>();
    let _ = m_copyback(sc.sc_mbuf.get(), off as i32, wire_bytes(flow), M_NOWAIT);

    pflowstat_inc(PflowstatCounters::PflowFlows);
    sc.sc_gcounter.set(sc.sc_gcounter.get().wrapping_add(1));
    sc.sc_count.set(sc.sc_count.get() + 1);

    if sc.sc_count.get() >= sc.sc_maxcount.get() {
        ret = pflow_sendout_v5(sc);
    }

    ret
}

/// `copy_flow_ipfix_4_to_m`: appends an IPv4 IPFIX record (`flow`, of template `tmpl`); a
/// full datagram goes out. Called with `sc_mtx` held.
pub fn copy_flow_ipfix_4_to_m(flow: &[u8], sc: &PflowSoftc, tmpl: u16) -> Result<(), Errno> {
    let mut ret = Ok(());
    let size = flow.len();

    mutex_assert_locked(&sc.sc_mtx, "copy_flow_ipfix_4_to_m");

    if tmpl == PFLOW_IPFIX_TMPL_NAT_IPV4_ID {
        if sc.sc_mbuf_nat.get().is_none() {
            let Some(m) = pflow_get_mbuf(sc, tmpl) else {
                return Err(Errno::ENOBUFS);
            };
            sc.sc_mbuf_nat.set(Some(m));
            sc.sc_count4_nat.set(0);
            let _ = timeout_add_sec(&sc.sc_tmo_nat, PFLOW_TIMEOUT);
        }
        let off = PFLOW_SET_HDRLEN + sc.sc_count4_nat.get() as usize * size;
        let _ = m_copyback(sc.sc_mbuf_nat.get(), off as i32, flow, M_NOWAIT);
        pflowstat_inc(PflowstatCounters::PflowFlows);
        sc.sc_gcounter.set(sc.sc_gcounter.get().wrapping_add(1));
        sc.sc_count4_nat.set(sc.sc_count4_nat.get() + 1);

        if sc.sc_count4_nat.get() >= sc.sc_maxcount4.get() {
            ret = pflow_sendout_ipfix(sc, AF_INET, size, tmpl);
        }
    } else {
        if sc.sc_mbuf.get().is_none() {
            let Some(m) = pflow_get_mbuf(sc, tmpl) else {
                return Err(Errno::ENOBUFS);
            };
            sc.sc_mbuf.set(Some(m));
            sc.sc_count4.set(0);
            let _ = timeout_add_sec(&sc.sc_tmo, PFLOW_TIMEOUT);
        }
        let off = PFLOW_SET_HDRLEN + sc.sc_count4.get() as usize * size;
        let _ = m_copyback(sc.sc_mbuf.get(), off as i32, flow, M_NOWAIT);
        pflowstat_inc(PflowstatCounters::PflowFlows);
        sc.sc_gcounter.set(sc.sc_gcounter.get().wrapping_add(1));
        sc.sc_count4.set(sc.sc_count4.get() + 1);

        if sc.sc_count4.get() >= sc.sc_maxcount4.get() {
            ret = pflow_sendout_ipfix(sc, AF_INET, size, tmpl);
        }
    }

    ret
}

/// `copy_flow_ipfix_6_to_m`: appends an IPv6 IPFIX record. Called with `sc_mtx` held.
pub fn copy_flow_ipfix_6_to_m(flow: &PflowIpfixFlow6, sc: &PflowSoftc) -> Result<(), Errno> {
    let mut ret = Ok(());
    let size = size_of::<PflowIpfixFlow6>();

    mutex_assert_locked(&sc.sc_mtx, "copy_flow_ipfix_6_to_m");

    if sc.sc_mbuf6.get().is_none() {
        let Some(m) = pflow_get_mbuf(sc, PFLOW_IPFIX_TMPL_IPV6_ID) else {
            return Err(Errno::ENOBUFS);
        };
        sc.sc_mbuf6.set(Some(m));
        sc.sc_count6.set(0);
        let _ = timeout_add_sec(&sc.sc_tmo6, PFLOW_TIMEOUT);
    }
    let off = PFLOW_SET_HDRLEN + sc.sc_count6.get() as usize * size;
    let _ = m_copyback(sc.sc_mbuf6.get(), off as i32, wire_bytes(flow), M_NOWAIT);

    pflowstat_inc(PflowstatCounters::PflowFlows);
    sc.sc_gcounter.set(sc.sc_gcounter.get().wrapping_add(1));
    sc.sc_count6.set(sc.sc_count6.get() + 1);

    if sc.sc_count6.get() >= sc.sc_maxcount6.get() {
        ret = pflow_sendout_ipfix(sc, AF_INET6, size, PFLOW_IPFIX_TMPL_IPV6_ID);
    }

    ret
}

/// `pflow_pack_flow`: the version 5 records of `st`, with the byte counters `bytes` (a
/// direction without bytes has no record).
pub fn pflow_pack_flow(
    st: &'static PfState,
    sk: &PfStateKey,
    sc: &PflowSoftc,
    bytes: [u64; 2],
) -> Result<(), Errno> {
    let mut flow1 = PflowFlow::default();
    let mut flow2 = PflowFlow::default();
    let mut ret = Ok(());

    if st.direction.get() == PF_OUT {
        copy_flow_data(&mut flow1, &mut flow2, st, sk, bytes, 1, 0);
    } else {
        copy_flow_data(&mut flow1, &mut flow2, st, sk, bytes, 0, 1);
    }

    if bytes[0] != 0 {
        // first flow from state
        ret = copy_flow_to_m(&flow1, sc);
    }

    if bytes[1] != 0 {
        // second flow from state
        ret = copy_flow_to_m(&flow2, sc);
    }

    ret
}

/// `pflow_pack_flow_ipfix`: the IPFIX records of `st` (NAT records when the keys differ).
pub fn pflow_pack_flow_ipfix(
    st: &'static PfState,
    sk: &PfStateKey,
    skw: &PfStateKey,
    sc: &PflowSoftc,
) -> Result<(), Errno> {
    let mut ret = Ok(());

    let is_nat = !ptr::eq(sk, skw);
    let (src, dst) = if st.direction.get() == PF_OUT {
        (1, 0)
    } else {
        (0, 1)
    };

    if sk.af.get() == AF_INET {
        if is_nat {
            let mut natflow4_1 = PflowIpfixNatFlow4::default();
            let mut natflow4_2 = PflowIpfixNatFlow4::default();

            copy_flow_ipfix_nat_4_data(&mut natflow4_1, &mut natflow4_2, st, sk, skw, sc, src, dst);

            if st.bytes[0].get() != 0 {
                // first flow from state
                ret = copy_flow_ipfix_4_to_m(
                    wire_bytes(&natflow4_1),
                    sc,
                    PFLOW_IPFIX_TMPL_NAT_IPV4_ID,
                );
            }
            if st.bytes[1].get() != 0 {
                // second flow from state
                ret = copy_flow_ipfix_4_to_m(
                    wire_bytes(&natflow4_2),
                    sc,
                    PFLOW_IPFIX_TMPL_NAT_IPV4_ID,
                );
            }
        } else {
            let mut flow4_1 = PflowIpfixFlow4::default();
            let mut flow4_2 = PflowIpfixFlow4::default();

            copy_flow_ipfix_4_data(&mut flow4_1, &mut flow4_2, st, sk, sc, src, dst);

            if st.bytes[0].get() != 0 {
                // first flow from state
                ret = copy_flow_ipfix_4_to_m(wire_bytes(&flow4_1), sc, PFLOW_IPFIX_TMPL_IPV4_ID);
            }
            if st.bytes[1].get() != 0 {
                // second flow from state
                ret = copy_flow_ipfix_4_to_m(wire_bytes(&flow4_2), sc, PFLOW_IPFIX_TMPL_IPV4_ID);
            }
        }
    } else if sk.af.get() == AF_INET6 {
        let mut flow6_1 = PflowIpfixFlow6::default();
        let mut flow6_2 = PflowIpfixFlow6::default();

        copy_flow_ipfix_6_data(&mut flow6_1, &mut flow6_2, st, sk, sc, src, dst);

        if st.bytes[0].get() != 0 {
            // first flow from state
            ret = copy_flow_ipfix_6_to_m(&flow6_1, sc);
        }

        if st.bytes[1].get() != 0 {
            // second flow from state
            ret = copy_flow_ipfix_6_to_m(&flow6_2, sc);
        }
    }
    ret
}

/// `pflow_timeout_nat`: `sc_tmo_nat`.
fn pflow_timeout_nat(v: *mut c_void) {
    let sc = PflowSoftc::of_arg(v);

    mtx_enter(&sc.sc_mtx);
    match sc.sc_version.get() {
        PFLOW_PROTO_5 => {
            let _ = pflow_sendout_v5(sc);
        }
        PFLOW_PROTO_10 => {
            let _ = pflow_sendout_ipfix(
                sc,
                AF_INET,
                size_of::<PflowIpfixNatFlow4>(),
                PFLOW_IPFIX_TMPL_NAT_IPV4_ID,
            );
        }
        _ => {} // NOTREACHED
    }
    mtx_leave(&sc.sc_mtx);
}

/// `pflow_timeout`: `sc_tmo`.
fn pflow_timeout(v: *mut c_void) {
    let sc = PflowSoftc::of_arg(v);

    mtx_enter(&sc.sc_mtx);
    match sc.sc_version.get() {
        PFLOW_PROTO_5 => {
            let _ = pflow_sendout_v5(sc);
        }
        PFLOW_PROTO_10 => {
            let _ = pflow_sendout_ipfix(
                sc,
                AF_INET,
                size_of::<PflowIpfixFlow4>(),
                PFLOW_IPFIX_TMPL_IPV4_ID,
            );
        }
        _ => {} // NOTREACHED
    }
    mtx_leave(&sc.sc_mtx);
}

/// `pflow_timeout6`: `sc_tmo6`.
fn pflow_timeout6(v: *mut c_void) {
    let sc = PflowSoftc::of_arg(v);

    mtx_enter(&sc.sc_mtx);
    let _ = pflow_sendout_ipfix(
        sc,
        AF_INET6,
        size_of::<PflowIpfixFlow6>(),
        PFLOW_IPFIX_TMPL_IPV6_ID,
    );
    mtx_leave(&sc.sc_mtx);
}

/// `pflow_timeout_tmpl`: `sc_tmo_tmpl`.
fn pflow_timeout_tmpl(v: *mut c_void) {
    let sc = PflowSoftc::of_arg(v);

    mtx_enter(&sc.sc_mtx);
    let _ = pflow_sendout_ipfix_tmpl(sc);
    mtx_leave(&sc.sc_mtx);
}

/// `pflow_flush`: sends what the buffers hold.
pub fn pflow_flush(sc: &PflowSoftc) {
    mtx_enter(&sc.sc_mtx);
    match sc.sc_version.get() {
        PFLOW_PROTO_5 => {
            let _ = pflow_sendout_v5(sc);
        }
        PFLOW_PROTO_10 => {
            let _ = pflow_sendout_ipfix(
                sc,
                AF_INET,
                size_of::<PflowIpfixNatFlow4>(),
                PFLOW_IPFIX_TMPL_NAT_IPV4_ID,
            );
            let _ = pflow_sendout_ipfix(
                sc,
                AF_INET,
                size_of::<PflowIpfixFlow4>(),
                PFLOW_IPFIX_TMPL_IPV4_ID,
            );
            let _ = pflow_sendout_ipfix(
                sc,
                AF_INET6,
                size_of::<PflowIpfixFlow6>(),
                PFLOW_IPFIX_TMPL_IPV6_ID,
            );
        }
        _ => {} // NOTREACHED
    }
    mtx_leave(&sc.sc_mtx);
}

/// Queues a finished datagram for `pflow_output_process`.
fn pflow_enqueue(sc: &PflowSoftc, m: &'static Mbuf) {
    let ifp = &sc.sc_if;
    // The softc lives until `pflow_clone_destroy`, which waits for the task to finish.
    let sc: &'static PflowSoftc = PflowSoftc::of_arg(sc.arg());
    if !mq_enqueue(&sc.sc_outputqueue, m)
        && let Some(tq) = net_tq(ifp.if_index.get())
    {
        let _ = task_add(tq, &sc.sc_outputtask);
    }
}

/// `pflow_sendout_v5`: finishes the version 5 datagram and queues it. Called with `sc_mtx`
/// held.
pub fn pflow_sendout_v5(sc: &PflowSoftc) -> Result<(), Errno> {
    let ifp = &sc.sc_if;

    mutex_assert_locked(&sc.sc_mtx, "pflow_sendout_v5");

    let _ = timeout_del(&sc.sc_tmo);

    let Some(m) = sc.sc_mbuf.take() else {
        return Ok(());
    };

    if ifp.if_flags.get() & IFF_RUNNING == 0 {
        m_freem(m);
        return Ok(());
    }

    pflowstat_inc(PflowstatCounters::PflowPackets);
    let mut h: PflowHeader = m_get_wire(m, 0);
    h.count = htons(sc.sc_count.get() as u16);

    // populate pflow_header
    h.uptime_ms = htonl((getuptime() * 1000) as u32);

    let tv = getnanotime();
    h.time_sec = htonl(tv.tv_sec as u32); // XXX 2038
    h.time_nanosec = htonl(tv.tv_nsec as u32);
    let _ = m_copyback(m, 0, wire_bytes(&h), M_NOWAIT);
    pflow_enqueue(sc, m);
    Ok(())
}

/// `pflow_sendout_ipfix`: finishes the IPFIX datagram of `af` (and template `tmpl`, records
/// of `size` bytes) and queues it. Called with `sc_mtx` held.
pub fn pflow_sendout_ipfix(
    sc: &PflowSoftc,
    af: SaFamily,
    size: usize,
    tmpl: u16,
) -> Result<(), Errno> {
    let ifp = &sc.sc_if;

    mutex_assert_locked(&sc.sc_mtx, "pflow_sendout_ipfix");

    let (m, count, set_length) = match af {
        AF_INET => {
            let (m, count) = if tmpl == PFLOW_IPFIX_TMPL_NAT_IPV4_ID {
                let _ = timeout_del(&sc.sc_tmo_nat);
                let Some(m) = sc.sc_mbuf_nat.take() else {
                    return Ok(());
                };
                (m, sc.sc_count4_nat.get())
            } else {
                let _ = timeout_del(&sc.sc_tmo);
                let Some(m) = sc.sc_mbuf.take() else {
                    return Ok(());
                };
                (m, sc.sc_count4.get())
            };
            (
                m,
                count,
                size_of::<PflowSetHeader>() + count as usize * size,
            )
        }
        AF_INET6 => {
            let _ = timeout_del(&sc.sc_tmo6);
            let Some(m) = sc.sc_mbuf6.take() else {
                return Ok(());
            };
            let count = sc.sc_count6.get();
            (
                m,
                count,
                size_of::<PflowSetHeader>() + sc.sc_count6.get() as usize * size,
            )
        }
        _ => unhandled_af(i32::from(af)),
    };

    if ifp.if_flags.get() & IFF_RUNNING == 0 {
        m_freem(m);
        return Ok(());
    }

    pflowstat_inc(PflowstatCounters::PflowPackets);
    let mut set_hdr: PflowSetHeader = m_get_wire(m, 0);
    set_hdr.set_length = htons(set_length as u16);
    let _ = m_copyback(m, 0, wire_bytes(&set_hdr), M_NOWAIT);

    // populate pflow_header
    let Some(m) = m_prepend(m, size_of::<PflowV10Header>() as i32, M_DONTWAIT) else {
        pflowstat_inc(PflowstatCounters::PflowOnomem);
        return Err(Errno::ENOBUFS);
    };
    let h10 = PflowV10Header {
        version: htons(u16::from(PFLOW_PROTO_10)),
        length: htons((PFLOW_IPFIX_HDRLEN + set_length) as u16),
        time_sec: htonl(gettime() as u32), // XXX 2038
        flow_sequence: htonl(sc.sc_sequence.get()),
        observation_dom: htonl(u32::from(PFLOW_ENGINE_TYPE)),
    };
    sc.sc_sequence.set(sc.sc_sequence.get().wrapping_add(count));
    let _ = m_copyback(m, 0, wire_bytes(&h10), M_NOWAIT);
    pflow_enqueue(sc, m);
    Ok(())
}

/// `pflow_sendout_ipfix_tmpl`: queues the template set and rearms its timeout. Called with
/// `sc_mtx` held.
pub fn pflow_sendout_ipfix_tmpl(sc: &PflowSoftc) -> Result<(), Errno> {
    let ifp = &sc.sc_if;

    mutex_assert_locked(&sc.sc_mtx, "pflow_sendout_ipfix_tmpl");

    let _ = timeout_del(&sc.sc_tmo_tmpl);

    if ifp.if_flags.get() & IFF_RUNNING == 0 {
        return Ok(());
    }
    let Some(m) = pflow_get_mbuf(sc, 0) else {
        return Ok(());
    };
    let tmpl = sc.sc_tmpl_ipfix.get();
    if m_copyback(m, 0, wire_bytes(&tmpl), M_NOWAIT).is_err() {
        m_freem(m);
        return Ok(());
    }
    pflowstat_inc(PflowstatCounters::PflowPackets);

    // populate pflow_header
    let Some(m) = m_prepend(m, size_of::<PflowV10Header>() as i32, M_DONTWAIT) else {
        pflowstat_inc(PflowstatCounters::PflowOnomem);
        return Err(Errno::ENOBUFS);
    };
    let h10 = PflowV10Header {
        version: htons(u16::from(PFLOW_PROTO_10)),
        length: htons((PFLOW_IPFIX_HDRLEN + size_of::<PflowIpfixTmpl>()) as u16),
        time_sec: htonl(gettime() as u32), // XXX 2038
        flow_sequence: htonl(sc.sc_sequence.get()),
        observation_dom: htonl(u32::from(PFLOW_ENGINE_TYPE)),
    };
    let _ = m_copyback(m, 0, wire_bytes(&h10), M_NOWAIT);

    let _ = timeout_add_sec(&sc.sc_tmo_tmpl, PFLOW_TMPL_TIMEOUT);
    pflow_enqueue(sc, m);
    Ok(())
}

/// `pflow_sendout_mbuf`: sends a datagram to the collector. Called with `sc_lock` held.
pub fn pflow_sendout_mbuf(sc: &PflowSoftc, m: &'static Mbuf) -> Result<(), Errno> {
    rw_assert_anylock(&sc.sc_lock);

    if let Some(c) = sc.sc_if.if_counters.get() {
        counters_pkt(
            c,
            IfCounters::IfcOpackets,
            IfCounters::IfcObytes,
            m.m_pkthdr().len.get() as u64,
        );
    }

    let Some(so) = sc.so.get() else {
        m_freem(m);
        return Err(Errno::EINVAL);
    };
    sosend(so, sc.send_nam.get(), None, Some(m), None, 0)
}

/// `pflow_sysctl`: `net.pflow`.
pub fn pflow_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    if name.len() != 1 {
        return Err(Errno::ENOTDIR);
    }

    match name[0] {
        NET_PFLOW_STATS => {
            if newp != 0 {
                return Err(Errno::EPERM);
            }

            let counters: [u64; PFLOW_NCOUNTERS] =
                core::array::from_fn(|i| PFLOW_COUNTERS[i].load(Ordering::Relaxed));

            let pflowstats = Pflowstats {
                pflow_flows: counters[PflowstatCounters::PflowFlows as usize],
                pflow_packets: counters[PflowstatCounters::PflowPackets as usize],
                pflow_onomem: counters[PflowstatCounters::PflowOnomem as usize],
                pflow_oerrors: counters[PflowstatCounters::PflowOerrors as usize],
            };

            let mut bytes = [0u8; size_of::<Pflowstats>()];
            for (i, v) in [
                pflowstats.pflow_flows,
                pflowstats.pflow_packets,
                pflowstats.pflow_onomem,
                pflowstats.pflow_oerrors,
            ]
            .iter()
            .enumerate()
            {
                bytes[i * 8..i * 8 + 8].copy_from_slice(&v.to_ne_bytes());
            }
            sysctl_struct(oldp, oldlenp, newp, newlen, &mut bytes)
        }
        _ => Err(Errno::EOPNOTSUPP),
    }
}

const _: () = {
    assert!(size_of::<PflowFlow>() == 48);
    assert!(size_of::<PflowSetHeader>() == 4);
    assert!(size_of::<PflowIpfixTmplIpv4>() == 52);
    assert!(size_of::<PflowIpfixTmplNatIpv4>() == 68);
    assert!(size_of::<PflowIpfixTmplIpv6>() == 52);
    assert!(size_of::<PflowIpfixTmpl>() == 176);
    assert!(size_of::<PflowIpfixFlow4>() == 54);
    assert!(size_of::<PflowIpfixNatFlow4>() == 66);
    assert!(size_of::<PflowIpfixFlow6>() == 78);
    assert!(size_of::<PflowHeader>() == 24);
    assert!(size_of::<PflowV10Header>() == 16);
    assert!(size_of::<Pflowstats>() == PFLOW_NCOUNTERS * size_of::<u64>());
    assert!(size_of::<Pflowreq>() == 520);
    assert!(offset_of!(Pflowreq, addrmask) == 512);
    assert!(size_of::<Udpiphdr>() == 28);
};

#[cfg(test)]
mod tests;
