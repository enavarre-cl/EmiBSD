/*	$OpenBSD: if_wg.h,v 1.7 2025/07/10 05:28:13 dlg Exp $ */
/*	$OpenBSD: if_wg.c,v 1.50 2026/09/20 21:18:09 mvs Exp $ */
/* <LICENSES> */

/*
 * Copyright (C) 2015-2020 Jason A. Donenfeld <Jason@zx2c4.com>. All Rights Reserved.
 * Copyright (C) 2019-2020 Matt Dunwoodie <ncon@noconroy.net>
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
/* </LICENSES> */

//! The WireGuard network interface, `wg(4)`: `<net/if_wg.h>` and `net/if_wg.c`.
//!
//! Upstream: sys/net/if_wg.h @ 3ce1f3f79392
//! Upstream: sys/net/if_wg.c @ 3ce1f3f79392
//!
//! `<net/if_wg.h>` is the public interface to the WireGuard network interface, used by tools
//! such as ifconfig(8) and wg(8): `SIOCSWG`/`SIOCGWG` carry a [`WgDataIo`] whose
//! `wgd_interface` points at a [`WgInterfaceIo`] followed by its peers ([`WgPeerIo`]), each
//! followed by its allowed IPs ([`WgAipIo`]).
//!
//! A `wg` interface ([`WgSoftc`], cloned by `ifconfig wg0 create`) owns a Noise identity
//! (`net/wg_noise.rs`), a cookie checker (`net/wg_cookie.rs`), a UDP socket per address
//! family and its peers ([`WgPeer`]): each with a Noise remote, a cookie maker, timers, an
//! endpoint and the allowed IPs, kept in an ART per family (`net/art.rs`) for the source
//! check of received packets and the choice of peer for sent ones.
//!
//! Packets go through two queues. `wg_output` tags a packet with its peer and enqueues it on
//! the interface; `wg_qstart` stages it on the peer and, once the peer has a session,
//! `wg_queue_out` puts it on the peer's serial queue and the interface's parallel ring. The
//! `wg_crypt` task queue encrypts from the ring (`wg_encap`), and the peer's `p_deliver_out`
//! task, on the interface's softnet queue, sends from the serial queue in order
//! (`wg_deliver_out`). Received datagrams arrive through the UDP socket's upcall (`wg_input`):
//! handshake messages go to the `wg_handshake` task queue (`wg_handshake`), data to the same
//! pair of queues the other way (`wg_decap`, `wg_deliver_in`, which hands the inner packet to
//! IPv4). The timers (`timeout(9)`) drive rekeying, keepalives and the erasure of old keys.
//!
//! Status: `ported` (M9b).
//!
//! ## Deviations
//! - The header and the file share this module. `INET6` is configured (feature `inet6`):
//!   `sc_so6` and `sc_aip6`, the endpoint's `sockaddr_in6` and `in6_pktinfo`, the IPv6 cases
//!   of `wg_input`, `wg_decap`, `wg_deliver_in`, `wg_output`, `wg_send`, `wg_bind` and
//!   `wg_aip_*`. The C unions are byte images with accessors: [`WgAipAddr`] (an `in_addr` or
//!   an `in6_addr`), [`WgPeerEndpoint`] (a `sockaddr`, `sockaddr_in` or `sockaddr_in6`, also
//!   the `e_remote` of a [`WgEndpoint`]) and [`WgLocal`] (an `in_addr` or an `in6_pktinfo`).
//!   Without the feature an IPv6 allowed IP is `EAFNOSUPPORT`, as the C's `default`. Holes the
//!   C compiler leaves are named `_pad*` fields.
//! - The cookie functions take the source address as a `&SockaddrStorage`
//!   (see `wg_cookie.rs`): [`WgPeerEndpoint::as_storage`] makes one.
//! - `NBPFILTER` and `NPF` are configured: `wg_clone_create` attaches a `DLT_LOOP` tap and
//!   `wg_deliver_in`/`wg_qstart` tap with `bpf_mtap_af`; `wg_decap` calls
//!   `pf_pkt_addr_changed`.
//! - [`wg_input`] is the UDP pcb's `inp_upcall` ([`InpUpcallFn`](crate::netinet::in_pcb::InpUpcallFn), an `unsafe fn`: the
//!   headers come as raw pointers); it reads `uh_sport`, `struct udphdr`'s first member,
//!   through the pointer.
//! - The `struct mbuf`s the C builds on its stack for `sobind`, `sosetopt` and `sosend`'s
//!   address (`mhostnam`, `mrtable`, `peernam`) are `m_get`'d and freed after the call (an
//!   mbuf is `&'static Mbuf`); `peernam` holds a copy of the address instead of pointing at
//!   it. When `M_WAIT` allocations fail anyway, the packet is dropped (`ENOBUFS`).
//! - Members changed through a shared pointer are `Cell`s; the softc is `malloc(M_ZERO)`ed and
//!   the peers and allowed IPs are `pool_get(PR_ZERO)`ed (the C initialises every member of a
//!   new peer by hand), so the all-zero value of each is valid; `sc_local` is written whole
//!   before the softc is shared (its upcall is an `Option`).
//! - `CONTAINER_OF` is `offset_of!` back from the member ([`WgPeer::of_remote`],
//!   [`WgPeer::of_timers`]); `(struct wg_aip *)node` relies on `a_node` being the first member
//!   of the `#[repr(C)]` [`WgAip`]. Timeouts and tasks take their structure as the `void *`.
//! - The wire structures are `#[repr(C)]` without padding; they are read out of the mbuf with
//!   an unaligned copy and written from their bytes. The packet type and indices keep the C's
//!   byte order (`htole32` constants, indices copied raw); the data nonce is little-endian.
//! - `wg_tag` lives in the `m_tag`'s data as `Cell`s (`t_done` a `bool`).
//! - `sockaddr_ntop` (`netinet/inet_ntop.c`) is not ported: the log lines format the address
//!   with [`SaNtop`], which prints what it would (`a.b.c.d`, an IPv6 address through
//!   `In6Ntop`, or the family and the bytes).
//! - `wg_timers_expired_handshake_last_sent`, `wg_timers_check_handshake_last_sent` answer
//!   `bool` (`ETIMEDOUT` is `true`); `wg_timers_get_persistent_keepalive`, the ioctls,
//!   `wg_aip_*`, `wg_send`, `wg_bind` and the cloner return `Result`.
//! - `wg_aip_add` refuses a negative `a_cidr` (`EINVAL`), which the C hands to
//!   `art_node_init` as a huge unsigned length, and an `art_insert` that cannot allocate
//!   (`ENOBUFS`), where the C would cast its NULL to a `wg_aip`.
//! - `wg_index_drop` of an index that is not in the table panics, where the C dereferences the
//!   NULL its search ends with.
//! - `wg_last_underload` is a `StaticCell` guarded by a mutex the C does not have
//!   (`wg_last_underload_mtx`): the C's function-static `struct timeval` is read and written
//!   unlocked by the two threads of `wg_handshake_taskq`, a race the C tolerates and Rust
//!   does not.
//! - `explicit_bzero(peer, sizeof(*peer))` before `pool_put` clears the peer's secrets (keys,
//!   handshake, cookie state) member by member.
//! - `WGTEST` (`cookie_test`, `noise_test` in `wgattach`) is the host test modules of
//!   `wg_cookie.rs` and `wg_noise.rs`.

use core::cell::Cell;
use core::ffi::c_void;
use core::fmt;
use core::mem::offset_of;
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicPtr, AtomicU64, AtomicUsize, Ordering};

use libkern::{explicit_bzero, strlcpy, timingsafe_bcmp};

use crate::crypto::curve25519::curve25519_generate_public;
use crate::crypto::siphash::{SipHash24, SiphashKey};
use crate::dev::rnd::{arc4random, arc4random_buf, arc4random_uniform};
use crate::kern::init_main::NCPUS;
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_init_flags, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_prot::suser;
use crate::kern::kern_rwlock::{
    rw_assert_wrlock, rw_enter_read, rw_enter_write, rw_exit_read, rw_exit_write, rw_init,
};
use crate::kern::kern_subr::{hashfree, hashinit};
use crate::kern::kern_synch::{NOWAKE, tsleep_nsec};
use crate::kern::kern_task::{task_add, task_set, taskq_barrier, taskq_create, taskq_destroy};
use crate::kern::kern_tc::{getmicrouptime, getnanotime, getnanouptime};
use crate::kern::kern_time::ratecheck;
use crate::kern::kern_timeout::{
    timeout_add_msec, timeout_add_sec, timeout_del, timeout_del_barrier, timeout_set,
};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::{Str, log, panic, snprintf};
use crate::kern::uipc_mbuf::{
    MAX_HDR, m_adj, m_align, m_clget, m_copydata, m_free, m_freem, m_get, m_gethdr, m_pullup,
    ml_dequeue, ml_enqueue, ml_init, ml_purge, mq_delist, mq_dequeue, mq_enqueue, mq_init,
    mq_purge, mq_push,
};
use crate::kern::uipc_mbuf2::{m_tag_find, m_tag_get, m_tag_prepend};
use crate::kern::uipc_socket::{sobind, soclose, socreate, sosend, sosetopt};
use crate::kern::uipc_socket2::{sbcreatecontrol, solock, sounlock};
use crate::machine::copy::{AbiPod, copyin_obj, copyout_obj};
use crate::machine::cpu::curproc;
use crate::machine::intr::IPL_NET;
use crate::net::art::{
    Art, ArtNode, art_alloc, art_delete, art_insert, art_lookup, art_match, art_node_init,
};
use crate::net::bpf::{BPF_DIRECTION_IN, BPF_DIRECTION_OUT, DLT_LOOP, bpf_mtap_af, bpfattach};
use crate::net::if_::{
    IFDESCRSIZE, IFF_BROADCAST, IFF_DEBUG, IFF_MULTICAST, IFF_NOARP, IFF_RUNNING, IFF_UP, IFNAMSIZ,
    IFQ_MAXPRIO, IFXF_CLONED, IFXF_MPSAFE, Ifreq, counters_inc, counters_pkt, if_alloc_sadl,
    if_attach, if_clone_attach, if_counters_alloc, if_detach, if_enqueue, net_tq, p2p_rtrequest,
};
use crate::net::if_types::IFT_WIREGUARD;
use crate::net::if_var::{IfClone, IfCounterArray, IfCounters, Ifnet, Netstack};
use crate::net::ifq::{Ifqueue, ifq_dequeue, ifq_empty, ifq_purge};
use crate::net::pf::pf_pkt_addr_changed;
use crate::net::route::Rtentry;
use crate::net::wg_cookie::{
    COOKIE_ENCRYPTED_SIZE, COOKIE_NONCE_SIZE, CookieChecker, CookieMacs, CookieMaker,
    RatelimitEntry, cookie_checker_create_payload, cookie_checker_deinit, cookie_checker_init,
    cookie_checker_update, cookie_checker_validate_macs, cookie_maker_consume_payload,
    cookie_maker_init, cookie_maker_mac,
};
use crate::net::wg_noise::{
    NOISE_AUTHTAG_LEN, NOISE_PUBLIC_KEY_LEN, NOISE_TIMESTAMP_LEN, NoiseHandshake, NoiseLocal,
    NoiseRemote, NoiseUpcall, REJECT_AFTER_TIME, noise_consume_initiation, noise_consume_response,
    noise_create_initiation, noise_create_response, noise_local_init, noise_local_keys,
    noise_local_lock_identity, noise_local_set_private, noise_local_unlock_identity,
    noise_remote_begin_session, noise_remote_clear, noise_remote_decrypt, noise_remote_encrypt,
    noise_remote_expire_current, noise_remote_init, noise_remote_keys, noise_remote_precompute,
    noise_remote_ready, noise_remote_set_psk,
};
#[cfg(feature = "inet6")]
use crate::netinet::in_::IPPROTO_IPV6;
use crate::netinet::in_::{INADDR_ANY, IP_SENDSRCADDR, IPPROTO_IP, InAddr, SockaddrIn};
use crate::netinet::in_pcb::sotoinpcb;
use crate::netinet::ip::{IPVERSION, Ip};
use crate::netinet::ip_input::ipv4_input;
#[cfg(feature = "inet6")]
use crate::netinet::ip6::{IPV6_VERSION, IPV6_VERSION_MASK, Ip6Hdr};
#[cfg(feature = "inet6")]
use crate::netinet6::in6::{IN6ADDR_ANY, IPV6_PKTINFO, in6_is_addr_unspecified};
use crate::netinet6::in6::{In6Addr, In6Pktinfo, SockaddrIn6};
#[cfg(feature = "inet6")]
use crate::netinet6::ip6_input::ipv6_input;
#[cfg(feature = "inet6")]
use crate::netinet6::nd6::In6Ntop;
use crate::queue_adapter;
use crate::sys::endian::{htons, ntohs};
use crate::sys::errno::Errno;
use crate::sys::ioccom::_iowr;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_RTABLE, M_ZERO};
use crate::sys::mbuf::{
    M_BCAST, M_DONTWAIT, M_EXT, M_MAXLOOP, M_MCAST, M_WAIT, MHLEN, MT_DATA, MT_SONAME, MT_SOOPTS,
    MTag, Mbuf, MbufList, MbufQueue, PACKET_TAG_MAXSIZE, PACKET_TAG_WIREGUARD, mq_empty, mq_len,
    mtod,
};
use crate::sys::mutex::Mutex;
use crate::sys::param::PWAIT;
use crate::sys::pool::{PR_NOWAIT, PR_ZERO, Pool};
use crate::sys::queue::{ListEntry, ListHead, SlistEntry, SlistHead, TailqEntry, TailqHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::socket::{
    AF_INET, AF_INET6, AF_UNSPEC, SO_RTABLE, SOCK_DGRAM, SOL_SOCKET, Sockaddr, SockaddrStorage,
};
use crate::sys::socketvar::Socket;
use crate::sys::sockio::{SIOCADDMULTI, SIOCDELMULTI, SIOCSIFADDR, SIOCSIFFLAGS, SIOCSIFMTU};
use crate::sys::syslog::{LOG_DEBUG, LOG_INFO, LOG_WARNING};
use crate::sys::systm::{kernel_assert_locked, net_assert_locked, net_lock, net_unlock};
use crate::sys::task::{TASKQ_MPSAFE, Task, Taskq};
use crate::sys::time::{Timespec, Timeval, timespecadd};
use crate::sys::timeout::{Timeout, timeout_pending};
use crate::sys::types::{InPort, SaFamily};
use libkern::StaticCell;

/// `WG_KEY_LEN`.
pub const WG_KEY_LEN: usize = 32;

// These ioctls do not need a NETLOCK as they use their own locks to serialise access.

/// `SIOCSWG`: configure the interface and its peers.
pub const SIOCSWG: u64 = _iowr::<WgDataIo>(b'i', 210);
/// `SIOCGWG`: read the configuration back.
pub const SIOCGWG: u64 = _iowr::<WgDataIo>(b'i', 211);

/// `sizeof(struct in6_addr)`.
const IN6_ADDR_LEN: usize = size_of::<In6Addr>();
/// `sizeof(struct sockaddr_in6)`.
const SOCKADDR_IN6_LEN: usize = size_of::<SockaddrIn6>();
/// `sizeof(struct in6_pktinfo)`.
const IN6_PKTINFO_LEN: usize = size_of::<In6Pktinfo>();

/// `union wg_aip_addr`: `addr_bytes` (the first byte of the others), `addr_ipv4` (`struct
/// in_addr`) and `addr_ipv6` (`struct in6_addr`), as the bytes of the longest.
#[repr(C, align(4))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WgAipAddr {
    /// The union's bytes, network order.
    pub addr_bytes: [u8; IN6_ADDR_LEN],
}

impl WgAipAddr {
    /// `a_ipv4` (`a_addr.addr_ipv4`).
    pub fn addr_ipv4(&self) -> InAddr {
        InAddr {
            s_addr: u32::from_ne_bytes([
                self.addr_bytes[0],
                self.addr_bytes[1],
                self.addr_bytes[2],
                self.addr_bytes[3],
            ]),
        }
    }

    /// Stores `a_ipv4`.
    pub fn set_addr_ipv4(&mut self, a: InAddr) {
        self.addr_bytes[..4].copy_from_slice(&a.s_addr.to_ne_bytes());
    }

    /// `a_ipv6` (`a_addr.addr_ipv6`).
    pub fn addr_ipv6(&self) -> In6Addr {
        In6Addr::new(self.addr_bytes)
    }

    /// Stores `a_ipv6`.
    pub fn set_addr_ipv6(&mut self, a: In6Addr) {
        self.addr_bytes = a.s6_addr;
    }
}

/// `struct wg_aip_io`: an allowed IP of a peer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WgAipIo {
    /// `a_af`.
    pub a_af: SaFamily,
    /// The C compiler's hole before `a_cidr`.
    pub _pad0: [u8; 3],
    /// `a_cidr`: the prefix length.
    pub a_cidr: i32,
    /// `a_addr`.
    pub a_addr: WgAipAddr,
}

// SAFETY: `repr(C)`, integers and byte arrays only, the hole after `a_af` is a named field: no
// implicit padding, and any bit pattern is a value.
unsafe impl AbiPod for WgAipIo {}

/// `WG_PEER_HAS_PUBLIC`.
pub const WG_PEER_HAS_PUBLIC: i32 = 1 << 0;
/// `WG_PEER_HAS_PSK`.
pub const WG_PEER_HAS_PSK: i32 = 1 << 1;
/// `WG_PEER_HAS_PKA`.
pub const WG_PEER_HAS_PKA: i32 = 1 << 2;
/// `WG_PEER_HAS_ENDPOINT`.
pub const WG_PEER_HAS_ENDPOINT: i32 = 1 << 3;
/// `WG_PEER_REPLACE_AIPS`.
pub const WG_PEER_REPLACE_AIPS: i32 = 1 << 4;
/// `WG_PEER_REMOVE`.
pub const WG_PEER_REMOVE: i32 = 1 << 5;
/// `WG_PEER_UPDATE`.
pub const WG_PEER_UPDATE: i32 = 1 << 6;
/// `WG_PEER_SET_DESCRIPTION`.
pub const WG_PEER_SET_DESCRIPTION: i32 = 1 << 7;

/// `union wg_peer_endpoint`: `sa_sa` (`struct sockaddr`), `sa_sin` (`struct sockaddr_in`) and
/// `sa_sin6` (`struct sockaddr_in6`), as the bytes of the longest.
#[repr(C, align(4))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WgPeerEndpoint {
    /// The union's bytes.
    pub sa_bytes: [u8; SOCKADDR_IN6_LEN],
}

impl WgPeerEndpoint {
    /// `p_sa`: the `struct sockaddr` view.
    pub fn sa_sa(&self) -> Sockaddr {
        // SAFETY: the union is longer than a `sockaddr` (integers only); unaligned read.
        unsafe { self.sa_bytes.as_ptr().cast::<Sockaddr>().read_unaligned() }
    }

    /// `p_sin`: the `struct sockaddr_in` view.
    pub fn sa_sin(&self) -> SockaddrIn {
        // SAFETY: the union is longer than a `sockaddr_in` (integers only); unaligned read.
        unsafe { self.sa_bytes.as_ptr().cast::<SockaddrIn>().read_unaligned() }
    }

    /// Stores a `struct sockaddr_in` (`memcpy(&p_sin, ...)`), the rest unchanged.
    pub fn set_sa_sin(&mut self, sin: &SockaddrIn) {
        // SAFETY: the union is longer than a `sockaddr_in`; unaligned write of a `Copy` value.
        unsafe {
            self.sa_bytes
                .as_mut_ptr()
                .cast::<SockaddrIn>()
                .write_unaligned(*sin)
        };
    }

    /// `p_sin6`: the `struct sockaddr_in6` view.
    pub fn sa_sin6(&self) -> SockaddrIn6 {
        // SAFETY: the union is as long as a `sockaddr_in6` (integers and bytes only);
        // unaligned read.
        unsafe {
            self.sa_bytes
                .as_ptr()
                .cast::<SockaddrIn6>()
                .read_unaligned()
        }
    }

    /// Stores a `struct sockaddr_in6`.
    pub fn set_sa_sin6(&mut self, sin6: &SockaddrIn6) {
        // SAFETY: the union is as long as a `sockaddr_in6`; unaligned write of a `Copy` value.
        unsafe {
            self.sa_bytes
                .as_mut_ptr()
                .cast::<SockaddrIn6>()
                .write_unaligned(*sin6)
        };
    }

    /// `p_sa.sa_len`.
    pub const fn sa_len(&self) -> u8 {
        self.sa_bytes[0]
    }

    /// `p_sa.sa_family`.
    pub const fn sa_family(&self) -> SaFamily {
        self.sa_bytes[1]
    }

    /// The address as a `struct sockaddr_storage` (the source address of the cookie
    /// functions): the union's bytes, the rest zero.
    pub fn as_storage(&self) -> SockaddrStorage {
        let mut ss = SockaddrStorage::zeroed();
        // SAFETY: a `sockaddr_storage` is longer than the union (asserted at the end of the
        // file) and made of integers; the bytes are copied over its start.
        unsafe {
            ptr::copy_nonoverlapping(
                self.sa_bytes.as_ptr(),
                ptr::from_mut(&mut ss).cast::<u8>(),
                SOCKADDR_IN6_LEN,
            )
        };
        ss
    }
}

impl Default for WgPeerEndpoint {
    fn default() -> Self {
        Self {
            sa_bytes: [0; SOCKADDR_IN6_LEN],
        }
    }
}

/// `struct wg_peer_io`: a peer, followed in memory by its `p_aips_count` allowed IPs.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WgPeerIo {
    /// `p_flags`: `WG_PEER_*`.
    pub p_flags: i32,
    /// `p_protocol_version`.
    pub p_protocol_version: i32,
    /// `p_public`.
    pub p_public: [u8; WG_KEY_LEN],
    /// `p_psk`.
    pub p_psk: [u8; WG_KEY_LEN],
    /// `p_pka`: the persistent keepalive interval.
    pub p_pka: u16,
    /// The C compiler's hole before `p_endpoint`.
    pub _pad0: [u8; 2],
    /// `p_endpoint` (`p_sa`, `p_sin`, `p_sin6`).
    pub p_endpoint: WgPeerEndpoint,
    /// `p_txbytes`.
    pub p_txbytes: u64,
    /// `p_rxbytes`.
    pub p_rxbytes: u64,
    /// `p_last_handshake`: nanotime.
    pub p_last_handshake: Timespec,
    /// `p_description`.
    pub p_description: [u8; IFDESCRSIZE],
    /// `p_aips_count`.
    pub p_aips_count: usize,
    // p_aips[]: the allowed IPs follow.
}

impl Default for WgPeerIo {
    fn default() -> Self {
        Self {
            p_flags: 0,
            p_protocol_version: 0,
            p_public: [0; WG_KEY_LEN],
            p_psk: [0; WG_KEY_LEN],
            p_pka: 0,
            _pad0: [0; 2],
            p_endpoint: WgPeerEndpoint::default(),
            p_txbytes: 0,
            p_rxbytes: 0,
            p_last_handshake: Timespec::default(),
            p_description: [0; IFDESCRSIZE],
            p_aips_count: 0,
        }
    }
}

// SAFETY: `repr(C)`, integers, byte arrays, a `Timespec` (itself `AbiPod`) and the 4-aligned
// union; the only hole (after `p_pka`) is a named field and the size is a multiple of 8: no
// implicit padding, any bit pattern is a value (checked by the assertions at the end).
unsafe impl AbiPod for WgPeerIo {}

/// `WG_INTERFACE_HAS_PUBLIC`.
pub const WG_INTERFACE_HAS_PUBLIC: u8 = 1 << 0;
/// `WG_INTERFACE_HAS_PRIVATE`.
pub const WG_INTERFACE_HAS_PRIVATE: u8 = 1 << 1;
/// `WG_INTERFACE_HAS_PORT`.
pub const WG_INTERFACE_HAS_PORT: u8 = 1 << 2;
/// `WG_INTERFACE_HAS_RTABLE`.
pub const WG_INTERFACE_HAS_RTABLE: u8 = 1 << 3;
/// `WG_INTERFACE_REPLACE_PEERS`.
pub const WG_INTERFACE_REPLACE_PEERS: u8 = 1 << 4;

/// `struct wg_interface_io`: the interface, followed in memory by its `i_peers_count` peers.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WgInterfaceIo {
    /// `i_flags`: `WG_INTERFACE_*`.
    pub i_flags: u8,
    /// The C compiler's hole before `i_port`.
    pub _pad0: u8,
    /// `i_port`: host order.
    pub i_port: InPort,
    /// `i_rtable`.
    pub i_rtable: i32,
    /// `i_public`.
    pub i_public: [u8; WG_KEY_LEN],
    /// `i_private`.
    pub i_private: [u8; WG_KEY_LEN],
    /// `i_peers_count`.
    pub i_peers_count: usize,
    // i_peers[]: the peers follow.
}

// SAFETY: `repr(C)`, integers and byte arrays, the one hole a named field, the size a multiple
// of the alignment: no implicit padding, any bit pattern is a value.
unsafe impl AbiPod for WgInterfaceIo {}

/// `struct wg_data_io`: the argument of `SIOCSWG`/`SIOCGWG`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WgDataIo {
    /// `wgd_name`: the interface.
    pub wgd_name: [u8; IFNAMSIZ],
    /// `wgd_size`: total size of the memory pointed to by `wgd_interface`.
    pub wgd_size: usize,
    /// `wgd_interface`: a user address (`struct wg_interface_io *`).
    pub wgd_interface: usize,
}

/// `DEFAULT_MTU`.
pub const DEFAULT_MTU: u32 = 1420;

/// `MAX_STAGED_PKT`.
pub const MAX_STAGED_PKT: u32 = 128;
/// `MAX_QUEUED_PKT`.
pub const MAX_QUEUED_PKT: u32 = 1024;
/// `MAX_QUEUED_PKT_MASK`.
pub const MAX_QUEUED_PKT_MASK: u32 = MAX_QUEUED_PKT - 1;

/// `MAX_QUEUED_HANDSHAKES`.
pub const MAX_QUEUED_HANDSHAKES: u32 = 4096;

/// `HASHTABLE_PEER_SIZE`.
pub const HASHTABLE_PEER_SIZE: i32 = 1 << 11;
/// `HASHTABLE_INDEX_SIZE`.
pub const HASHTABLE_INDEX_SIZE: i32 = 1 << 13;
/// `MAX_PEERS_PER_IFACE`.
pub const MAX_PEERS_PER_IFACE: usize = 1 << 20;

/// `REKEY_TIMEOUT` (seconds).
pub const REKEY_TIMEOUT: i32 = 5;
/// `REKEY_TIMEOUT_JITTER`: 1/3 sec, round for `arc4random_uniform`.
pub const REKEY_TIMEOUT_JITTER: u32 = 334;
/// `KEEPALIVE_TIMEOUT` (seconds).
pub const KEEPALIVE_TIMEOUT: i32 = 10;
/// `MAX_TIMER_HANDSHAKES`.
pub const MAX_TIMER_HANDSHAKES: i32 = 90 / REKEY_TIMEOUT;
/// `NEW_HANDSHAKE_TIMEOUT` (seconds).
pub const NEW_HANDSHAKE_TIMEOUT: i32 = REKEY_TIMEOUT + KEEPALIVE_TIMEOUT;
/// `UNDERLOAD_TIMEOUT` (seconds).
pub const UNDERLOAD_TIMEOUT: i64 = 1;

/// `WGPRINTF(loglevel, sc, mtx, fmt, ...)`: logs, prefixed by the interface name, when the
/// interface has `IFF_DEBUG`; the arguments are formatted with `mtx` held, if any.
macro_rules! wgprintf {
    ($level:expr, $sc:expr, $mtx:expr, $($arg:tt)*) => {{
        let sc: &WgSoftc = $sc;
        if sc.sc_if.if_flags.get() & IFF_DEBUG != 0 {
            let mtx: Option<&Mutex> = $mtx;
            if let Some(m) = mtx {
                mtx_enter(m);
            }
            let xname = sc.sc_if.if_xname.get();
            log(
                $level,
                format_args!("{}: {}", Str(cstr(&xname)), format_args!($($arg)*)),
            );
            if let Some(m) = mtx {
                mtx_leave(m);
            }
        }
    }};
}

// First byte indicating packet type on the wire

/// `WG_PKT_INITIATION`.
pub const WG_PKT_INITIATION: u32 = 1u32.to_le();
/// `WG_PKT_RESPONSE`.
pub const WG_PKT_RESPONSE: u32 = 2u32.to_le();
/// `WG_PKT_COOKIE`.
pub const WG_PKT_COOKIE: u32 = 3u32.to_le();
/// `WG_PKT_DATA`.
pub const WG_PKT_DATA: u32 = 4u32.to_le();

/// `WG_PKT_WITH_PADDING(n)`: `n` rounded up to 16.
pub const fn wg_pkt_with_padding(n: usize) -> usize {
    (n + (16 - 1)) & !(16 - 1)
}

/// `WG_KEY_SIZE`.
pub const WG_KEY_SIZE: usize = WG_KEY_LEN;

/// A wire structure: `#[repr(C)]`, made of integers and byte arrays, without padding.
///
/// # Safety
///
/// Implement only for such types: every byte of a value is initialised and every bit pattern
/// is a value.
unsafe trait WgPkt: Copy {}

/// The bytes of a wire structure.
fn pkt_bytes<T: WgPkt>(pkt: &T) -> &[u8] {
    // SAFETY: `T: WgPkt` has no padding, so its `size_of::<T>()` bytes are initialised and may
    // be read as `u8`s while `pkt` is borrowed.
    unsafe { slice::from_raw_parts(ptr::from_ref(pkt).cast::<u8>(), size_of::<T>()) }
}

/// `struct wg_pkt_initiation`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WgPktInitiation {
    /// `t`.
    pub t: u32,
    /// `s_idx`.
    pub s_idx: u32,
    /// `ue`.
    pub ue: [u8; NOISE_PUBLIC_KEY_LEN],
    /// `es`.
    pub es: [u8; NOISE_PUBLIC_KEY_LEN + NOISE_AUTHTAG_LEN],
    /// `ets`.
    pub ets: [u8; NOISE_TIMESTAMP_LEN + NOISE_AUTHTAG_LEN],
    /// `m`.
    pub m: CookieMacs,
}

impl WgPktInitiation {
    /// An all-zero message.
    pub const fn zeroed() -> Self {
        Self {
            t: 0,
            s_idx: 0,
            ue: [0; NOISE_PUBLIC_KEY_LEN],
            es: [0; NOISE_PUBLIC_KEY_LEN + NOISE_AUTHTAG_LEN],
            ets: [0; NOISE_TIMESTAMP_LEN + NOISE_AUTHTAG_LEN],
            m: CookieMacs {
                mac1: [0; 16],
                mac2: [0; 16],
            },
        }
    }
}

// SAFETY: `repr(C)`: two `u32`s, byte arrays and `CookieMacs` (two byte arrays), 148 bytes
// with 4-byte alignment: no padding.
unsafe impl WgPkt for WgPktInitiation {}

/// `struct wg_pkt_response`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WgPktResponse {
    /// `t`.
    pub t: u32,
    /// `s_idx`.
    pub s_idx: u32,
    /// `r_idx`.
    pub r_idx: u32,
    /// `ue`.
    pub ue: [u8; NOISE_PUBLIC_KEY_LEN],
    /// `en`.
    pub en: [u8; NOISE_AUTHTAG_LEN],
    /// `m`.
    pub m: CookieMacs,
}

// SAFETY: `repr(C)`: three `u32`s and byte arrays, 92 bytes with 4-byte alignment: no padding.
unsafe impl WgPkt for WgPktResponse {}

/// `struct wg_pkt_cookie`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WgPktCookie {
    /// `t`.
    pub t: u32,
    /// `r_idx`.
    pub r_idx: u32,
    /// `nonce`.
    pub nonce: [u8; COOKIE_NONCE_SIZE],
    /// `ec`.
    pub ec: [u8; COOKIE_ENCRYPTED_SIZE],
}

// SAFETY: `repr(C)`: two `u32`s and byte arrays, 64 bytes with 4-byte alignment: no padding.
unsafe impl WgPkt for WgPktCookie {}

/// `struct wg_pkt_data`: the header of a data message; `buf[]` (the ciphertext and its tag)
/// follows.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WgPktData {
    /// `t`.
    pub t: u32,
    /// `r_idx`.
    pub r_idx: u32,
    /// `nonce`: little-endian.
    pub nonce: [u8; size_of::<u64>()],
}

// SAFETY: `repr(C)`: two `u32`s and a byte array, 16 bytes: no padding.
unsafe impl WgPkt for WgPktData {}

/// `union wg_local` (`e_local`): `l_in` (`struct in_addr`) and `l_pktinfo6` (`struct
/// in6_pktinfo`, `l_in6` its address), as the bytes of the longest.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WgLocal {
    bytes: [u8; IN6_PKTINFO_LEN],
}

impl WgLocal {
    /// `l_in`.
    pub fn l_in(&self) -> InAddr {
        InAddr {
            s_addr: u32::from_ne_bytes([
                self.bytes[0],
                self.bytes[1],
                self.bytes[2],
                self.bytes[3],
            ]),
        }
    }

    /// Stores `l_in`, the rest unchanged.
    pub fn set_l_in(&mut self, a: InAddr) {
        self.bytes[..4].copy_from_slice(&a.s_addr.to_ne_bytes());
    }

    /// `l_in6` (`l_pktinfo6.ipi6_addr`).
    pub fn l_in6(&self) -> In6Addr {
        let mut a = [0u8; IN6_ADDR_LEN];
        a.copy_from_slice(&self.bytes[..IN6_ADDR_LEN]);
        In6Addr::new(a)
    }

    /// Stores `l_in6`, the rest unchanged.
    pub fn set_l_in6(&mut self, a: In6Addr) {
        self.bytes[..IN6_ADDR_LEN].copy_from_slice(&a.s6_addr);
    }

    /// The union's bytes: `l_pktinfo6`, which is what the `IPV6_PKTINFO` control message
    /// carries (`l_in` is its first four bytes).
    pub const fn as_bytes(&self) -> &[u8; IN6_PKTINFO_LEN] {
        &self.bytes
    }
}

/// `struct wg_endpoint`: a peer's address (`e_remote`: the `r_sa`, `r_sin` and `r_sin6` views
/// of a [`WgPeerEndpoint`]) and the local address to send from (`e_local`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WgEndpoint {
    /// `e_remote`.
    pub e_remote: WgPeerEndpoint,
    /// `e_local`.
    pub e_local: WgLocal,
}

/// `struct wg_tag`: what a packet carries through the queues, in its `PACKET_TAG_WIREGUARD`
/// tag.
pub struct WgTag {
    /// `t_endpoint`: where a received packet came from.
    pub t_endpoint: Cell<WgEndpoint>,
    /// `t_peer`.
    pub t_peer: Cell<Option<&'static WgPeer>>,
    /// `t_mbuf`: the encrypted (or decrypted) packet; NULL when that failed.
    pub t_mbuf: Cell<Option<&'static Mbuf>>,
    /// `t_done`: the parallel queue is done with the packet.
    pub t_done: Cell<bool>,
    /// `t_mtu`.
    pub t_mtu: Cell<i32>,
}

/// `struct wg_index`: a session index of a peer.
pub struct WgIndex {
    /// `i_entry`: in `sc_index`.
    pub i_entry: ListEntry<WgIndex>,
    /// `i_unused_entry`: in `p_unused_index`.
    pub i_unused_entry: SlistEntry<WgIndex>,
    /// `i_key`.
    pub i_key: Cell<u32>,
    /// `i_value`.
    pub i_value: Cell<Option<&'static NoiseRemote>>,
}

queue_adapter!(
    /// `LIST_HEAD(,wg_index)`: a bucket of `sc_index`.
    pub WgIndexes: WgIndex, i_entry => ListEntry<WgIndex>
);

queue_adapter!(
    /// `SLIST_HEAD(,wg_index)`: `p_unused_index`.
    pub WgUnusedIndexes: WgIndex, i_unused_entry => SlistEntry<WgIndex>
);

/// `struct wg_timers`: the timers of a peer.
pub struct WgTimers {
    /// `t_mtx`: for blocking `wg_timers_event_*` when setting `t_disabled`.
    pub t_mtx: Mutex,

    /// `t_disabled`. Protected by: `t_mtx`.
    pub t_disabled: Cell<bool>,
    /// `t_need_another_keepalive`.
    pub t_need_another_keepalive: Cell<bool>,
    /// `t_persistent_keepalive_interval`.
    pub t_persistent_keepalive_interval: Cell<u16>,
    /// `t_new_handshake`.
    pub t_new_handshake: Timeout,
    /// `t_send_keepalive`.
    pub t_send_keepalive: Timeout,
    /// `t_retry_handshake`.
    pub t_retry_handshake: Timeout,
    /// `t_zero_key_material`.
    pub t_zero_key_material: Timeout,
    /// `t_persistent_keepalive`.
    pub t_persistent_keepalive: Timeout,

    /// `t_handshake_mtx`.
    pub t_handshake_mtx: Mutex,
    /// `t_handshake_last_sent`: nanouptime. Protected by: `t_handshake_mtx`.
    pub t_handshake_last_sent: Cell<Timespec>,
    /// `t_handshake_complete`: nanotime. Protected by: `t_handshake_mtx`.
    pub t_handshake_complete: Cell<Timespec>,
    /// `t_handshake_retries`. Protected by: `t_handshake_mtx`.
    pub t_handshake_retries: Cell<i32>,
}

/// `struct wg_aip`: an allowed IP, a node of the interface's ART.
#[repr(C)]
pub struct WgAip {
    /// `a_node`: first, so that the tree's node is the allowed IP.
    pub a_node: ArtNode,
    /// `a_entry`: in the peer's `p_aip`.
    pub a_entry: ListEntry<WgAip>,
    /// `a_peer`.
    pub a_peer: Cell<Option<&'static WgPeer>>,
    /// `a_data`.
    pub a_data: Cell<WgAipIo>,
}

impl WgAip {
    /// `(struct wg_aip *)node`.
    fn of_node(node: &'static ArtNode) -> &'static WgAip {
        // SAFETY: the interface's trees hold only the `a_node`s of `WgAip`s (`wg_aip_add`), the
        // first member of a `#[repr(C)]` structure, so the node's address is the allowed IP's.
        unsafe { &*ptr::from_ref(node).cast::<WgAip>() }
    }
}

queue_adapter!(
    /// `LIST_HEAD(,wg_aip)`: `p_aip`.
    pub WgAips: WgAip, a_entry => ListEntry<WgAip>
);

/// `struct wg_queue`: a peer's serial queue.
pub struct WgQueue {
    /// `q_mtx`.
    pub q_mtx: Mutex,
    /// `q_list`. Protected by: `q_mtx`.
    pub q_list: MbufList,
}

/// `struct wg_ring`: the interface's parallel queue.
pub struct WgRing {
    /// `r_mtx`.
    pub r_mtx: Mutex,
    /// `r_head`. Protected by: `r_mtx`.
    pub r_head: Cell<u32>,
    /// `r_tail`. Protected by: `r_mtx`.
    pub r_tail: Cell<u32>,
    /// `r_buf`. Protected by: `r_mtx`.
    pub r_buf: [Cell<Option<&'static Mbuf>>; MAX_QUEUED_PKT as usize],
}

/// `struct wg_peer`.
pub struct WgPeer {
    /// `p_pubkey_entry`: in `sc_peer`.
    pub p_pubkey_entry: ListEntry<WgPeer>,
    /// `p_seq_entry`: in `sc_peer_seq`.
    pub p_seq_entry: TailqEntry<WgPeer>,
    /// `p_id`.
    pub p_id: Cell<u64>,
    /// `p_sc`.
    pub p_sc: Cell<Option<&'static WgSoftc>>,

    /// `p_remote`.
    pub p_remote: NoiseRemote,
    /// `p_cookie`.
    pub p_cookie: CookieMaker,
    /// `p_timers`.
    pub p_timers: WgTimers,

    /// `p_counters_mtx`.
    pub p_counters_mtx: Mutex,
    /// `p_counters_tx`. Protected by: `p_counters_mtx`.
    pub p_counters_tx: Cell<u64>,
    /// `p_counters_rx`. Protected by: `p_counters_mtx`.
    pub p_counters_rx: Cell<u64>,

    /// `p_endpoint_mtx`.
    pub p_endpoint_mtx: Mutex,
    /// `p_endpoint`. Protected by: `p_endpoint_mtx` (read without it where the C does).
    pub p_endpoint: Cell<WgEndpoint>,

    /// `p_send_initiation`.
    pub p_send_initiation: Task,
    /// `p_send_keepalive`.
    pub p_send_keepalive: Task,
    /// `p_clear_secrets`.
    pub p_clear_secrets: Task,
    /// `p_deliver_out`.
    pub p_deliver_out: Task,
    /// `p_deliver_in`.
    pub p_deliver_in: Task,

    /// `p_stage_queue`.
    pub p_stage_queue: MbufQueue,
    /// `p_encap_queue`.
    pub p_encap_queue: WgQueue,
    /// `p_decap_queue`.
    pub p_decap_queue: WgQueue,

    /// `p_unused_index`.
    pub p_unused_index: SlistHead<WgUnusedIndexes>,
    /// `p_index`.
    pub p_index: [WgIndex; 3],

    /// `p_aip`. Protected by: `sc_aip_lock`.
    pub p_aip: ListHead<WgAips>,

    /// `p_start_list`: on `wg_qstart`'s list.
    pub p_start_list: SlistEntry<WgPeer>,
    /// `p_start_onlist`.
    pub p_start_onlist: Cell<bool>,

    /// `p_description`.
    pub p_description: Cell<[u8; IFDESCRSIZE]>,
}

// SAFETY: every member is set up by `wg_peer_create` before the peer is linked, and changed
// afterwards under the lock its documentation (or the C's) names, or by the one task or timeout
// that owns it.
unsafe impl Sync for WgPeer {}

impl WgPeer {
    /// `CONTAINER_OF(remote, struct wg_peer, p_remote)`.
    fn of_remote(remote: &NoiseRemote) -> &'static WgPeer {
        let p = ptr::from_ref(remote)
            .cast::<u8>()
            .wrapping_sub(offset_of!(WgPeer, p_remote));
        // SAFETY: every remote this interface hands to Noise (`noise_remote_init` in
        // `wg_peer_create`) is the `p_remote` of a pool-allocated peer, which outlives its
        // indices and handshakes (`wg_peer_destroy` drops them first).
        unsafe { &*p.cast::<WgPeer>() }
    }

    /// `CONTAINER_OF(t, struct wg_peer, p_timers)`.
    fn of_timers(t: &WgTimers) -> &'static WgPeer {
        let p = ptr::from_ref(t)
            .cast::<u8>()
            .wrapping_sub(offset_of!(WgPeer, p_timers));
        // SAFETY: every `WgTimers` is the `p_timers` of a pool-allocated peer, whose timeouts
        // are deleted (`wg_timers_disable`) before it is freed.
        unsafe { &*p.cast::<WgPeer>() }
    }

    /// `peer->p_sc`.
    fn sc(&self) -> &'static WgSoftc {
        match self.p_sc.get() {
            Some(sc) => sc,
            None => panic(format_args!("wg peer {:p}: no softc", self)),
        }
    }
}

queue_adapter!(
    /// `LIST_HEAD(,wg_peer)`: a bucket of `sc_peer`.
    pub WgPeersByKey: WgPeer, p_pubkey_entry => ListEntry<WgPeer>
);

queue_adapter!(
    /// `TAILQ_HEAD(,wg_peer)`: `sc_peer_seq`.
    pub WgPeerSeq: WgPeer, p_seq_entry => TailqEntry<WgPeer>
);

queue_adapter!(
    /// `SLIST_HEAD(,wg_peer)`: `wg_qstart`'s `start_list`.
    pub WgPeerStart: WgPeer, p_start_list => SlistEntry<WgPeer>
);

/// `struct wg_softc`.
#[repr(C)]
pub struct WgSoftc {
    /// `sc_if`: first, as the C's `ifp->if_softc` and the softc share an address.
    pub sc_if: Ifnet,
    /// `sc_secret`: the key of the peer hash.
    pub sc_secret: Cell<SiphashKey>,

    /// `sc_lock`.
    pub sc_lock: Rwlock,
    /// `sc_local`.
    pub sc_local: NoiseLocal,
    /// `sc_cookie`.
    pub sc_cookie: CookieChecker,
    /// `sc_udp_port`: network order. Protected by: `sc_lock`.
    pub sc_udp_port: Cell<InPort>,
    /// `sc_udp_rtable`. Protected by: `sc_lock`.
    pub sc_udp_rtable: Cell<i32>,

    /// `sc_so_lock`.
    pub sc_so_lock: Rwlock,
    /// `sc_so4`. Protected by: `sc_so_lock`.
    pub sc_so4: Cell<Option<&'static Socket>>,
    /// `sc_so6`. Protected by: `sc_so_lock`.
    #[cfg(feature = "inet6")]
    pub sc_so6: Cell<Option<&'static Socket>>,
    /// `sc_aip_lock`.
    pub sc_aip_lock: Rwlock,
    /// `sc_aip_num`. Protected by: `sc_aip_lock`.
    pub sc_aip_num: Cell<usize>,
    /// `sc_aip4`.
    pub sc_aip4: Cell<Option<&'static Art>>,
    /// `sc_aip6`.
    #[cfg(feature = "inet6")]
    pub sc_aip6: Cell<Option<&'static Art>>,
    /// `sc_peer_lock`.
    pub sc_peer_lock: Rwlock,
    /// `sc_peer_num`. Protected by: `sc_peer_lock`.
    pub sc_peer_num: Cell<usize>,
    /// `sc_peer`: the peers by public key. Protected by: `sc_peer_lock`.
    pub sc_peer: Cell<Option<&'static [ListHead<WgPeersByKey>]>>,
    /// `sc_peer_seq`: the peers in creation order. Protected by: `sc_peer_lock`.
    pub sc_peer_seq: TailqHead<WgPeerSeq>,
    /// `sc_peer_mask`.
    pub sc_peer_mask: Cell<u64>,

    /// `sc_index_mtx`.
    pub sc_index_mtx: Mutex,
    /// `sc_index`: the session indices. Protected by: `sc_index_mtx`.
    pub sc_index: Cell<Option<&'static [ListHead<WgIndexes>]>>,
    /// `sc_index_mask`.
    pub sc_index_mask: Cell<u64>,

    /// `sc_handshake`.
    pub sc_handshake: Task,
    /// `sc_handshake_queue`.
    pub sc_handshake_queue: MbufQueue,

    /// `sc_encap`.
    pub sc_encap: Task,
    /// `sc_decap`.
    pub sc_decap: Task,
    /// `sc_encap_ring`.
    pub sc_encap_ring: WgRing,
    /// `sc_decap_ring`.
    pub sc_decap_ring: WgRing,
}

// SAFETY: as for `WgPeer`: set up by `wg_clone_create` before `if_attach` publishes it, then
// changed under the locks named on the members.
unsafe impl Sync for WgSoftc {}

impl WgSoftc {
    /// `ifp->if_softc` of a `wg` interface.
    fn of_ifp(ifp: &Ifnet) -> &'static WgSoftc {
        // SAFETY: `wg_clone_create` points `if_softc` at the softc that embeds the interface;
        // only `wg` interfaces get these functions as their hooks, and the softc lives until
        // `wg_clone_destroy`, after `if_detach`.
        unsafe { &*ifp.if_softc.get().cast::<WgSoftc>() }
    }

    /// `sc->sc_aip4`, made by `wg_clone_create`.
    fn aip4(&self) -> &'static Art {
        match self.sc_aip4.get() {
            Some(art) => art,
            None => panic(format_args!("wg {:p}: no aip table", self)),
        }
    }

    /// `sc->sc_aip6`, made by `wg_clone_create`.
    #[cfg(feature = "inet6")]
    fn aip6(&self) -> &'static Art {
        match self.sc_aip6.get() {
            Some(art) => art,
            None => panic(format_args!("wg {:p}: no aip6 table", self)),
        }
    }
}

/// `sockaddr_ntop(sa, buf, len)`, as a `Display` (see the module's deviations).
pub struct SaNtop(WgPeerEndpoint);

impl SaNtop {
    /// The text of a remote address.
    pub fn of(e: &WgPeerEndpoint) -> Self {
        Self(*e)
    }
}

impl fmt::Display for SaNtop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let e = &self.0;
        if e.sa_len() < 2 {
            return f.write_str("bad sa");
        }
        if e.sa_family() == AF_INET {
            let b = e.sa_sin().sin_addr.s_addr.to_ne_bytes();
            return write!(f, "{}.{}.{}.{}", b[0], b[1], b[2], b[3]);
        }
        #[cfg(feature = "inet6")]
        if e.sa_family() == AF_INET6 {
            return write!(f, "{}", In6Ntop(e.sa_sin6().sin6_addr));
        }
        write!(f, "{} ", e.sa_family())?;
        let sa = e.sa_sa();
        let n = (usize::from(sa.sa_len) - 2).min(sa.sa_data.len());
        for b in &sa.sa_data[..n] {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

/// `peer_counter`.
pub static PEER_COUNTER: AtomicU64 = AtomicU64::new(0);
/// `wg_aip_pool`.
pub static WG_AIP_POOL: Pool = Pool::new();
/// `wg_peer_pool`.
pub static WG_PEER_POOL: Pool = Pool::new();
/// `wg_ratelimit_pool`.
pub static WG_RATELIMIT_POOL: Pool = Pool::new();
/// `underload_interval`.
pub static UNDERLOAD_INTERVAL: Timeval = Timeval::new(UNDERLOAD_TIMEOUT, 0);

/// `wg_counter`: the `wg` interfaces that exist (kernel lock).
pub static WG_COUNTER: AtomicUsize = AtomicUsize::new(0);
/// `wg_handshake_taskq`.
pub static WG_HANDSHAKE_TASKQ: AtomicPtr<Taskq> = AtomicPtr::new(ptr::null_mut());
/// `wg_crypt_taskq`.
pub static WG_CRYPT_TASKQ: AtomicPtr<Taskq> = AtomicPtr::new(ptr::null_mut());

/// `wg_cloner`.
pub static WG_CLONER: IfClone = IfClone::new(b"wg", wg_clone_create, Some(wg_clone_destroy));

/// `wg_handshake`'s `static struct timeval wg_last_underload` (microuptime; see the module's
/// deviations). Protected by: `WG_LAST_UNDERLOAD_MTX`.
static WG_LAST_UNDERLOAD: StaticCell<Timeval> = StaticCell::new(Timeval::new(0, 0));
/// Guards `WG_LAST_UNDERLOAD` (not in the C, see the module's deviations).
static WG_LAST_UNDERLOAD_MTX: Mutex = Mutex::new(IPL_NET);

/// The interface name up to its NUL.
fn cstr(s: &[u8]) -> &[u8] {
    s.split(|&b| b == 0).next().unwrap_or(s)
}

/// A task queue `wg_clone_create` made.
fn taskq(tq: &AtomicPtr<Taskq>) -> &'static Taskq {
    // SAFETY: the queues are created by the first `wg_clone_create` and destroyed by the last
    // `wg_clone_destroy`, after every peer (whose tasks and timeouts use them) is gone.
    match unsafe { tq.load(Ordering::Acquire).as_ref() } {
        Some(tq) => tq,
        None => panic(format_args!("wg: task queue not created")),
    }
}

/// `wg_handshake_taskq`.
fn wg_handshake_taskq() -> &'static Taskq {
    taskq(&WG_HANDSHAKE_TASKQ)
}

/// `wg_crypt_taskq`.
fn wg_crypt_taskq() -> &'static Taskq {
    taskq(&WG_CRYPT_TASKQ)
}

/// `counters_add(ifp->if_counters, c, n)`.
fn counters_add(ifp: &Ifnet, c: IfCounters, n: u64) {
    if let Some(ctrs) = ifp.if_counters.get() {
        let ctrs: &IfCounterArray = ctrs;
        ctrs[c as usize].fetch_add(n, Ordering::Relaxed);
    }
}

/// `counters_inc(ifp->if_counters, c)`.
fn ifp_counters_inc(ifp: &Ifnet, c: IfCounters) {
    if let Some(ctrs) = ifp.if_counters.get() {
        counters_inc(ctrs, c);
    }
}

/// The address of a peer as the hash and the trees see it: the `in_addr`'s bytes.
fn in_bytes(a: &InAddr) -> [u8; 4] {
    a.s_addr.to_ne_bytes()
}

/// `wg_peer_create`: a new peer with static public key `public`, or `None` when the interface
/// is full or memory is short. Called with `sc_lock` held for writing.
pub fn wg_peer_create(sc: &'static WgSoftc, public: &[u8; WG_KEY_SIZE]) -> Option<&'static WgPeer> {
    rw_assert_wrlock(&sc.sc_lock);

    if sc.sc_peer_num.get() >= MAX_PEERS_PER_IFACE {
        return None;
    }

    let p = pool_get(&WG_PEER_POOL, PR_NOWAIT | PR_ZERO)?.cast::<WgPeer>();
    // SAFETY: a zero-filled pool item of `size_of::<WgPeer>()` bytes, aligned for it; the
    // all-zero `WgPeer` is valid (see the module's deviations). It lives until
    // `wg_peer_destroy` gives it back.
    let peer: &'static WgPeer = unsafe { p.as_ref() };

    peer.p_id.set(PEER_COUNTER.fetch_add(1, Ordering::Relaxed));
    peer.p_sc.set(Some(sc));

    noise_remote_init(&peer.p_remote, public, &sc.sc_local);
    cookie_maker_init(&peer.p_cookie, public);
    wg_timers_init(&peer.p_timers);

    mtx_init(&peer.p_counters_mtx, IPL_NET);
    peer.p_counters_tx.set(0);
    peer.p_counters_rx.set(0);

    let mut descr = [0u8; IFDESCRSIZE];
    strlcpy(&mut descr, b"\0");
    peer.p_description.set(descr);

    mtx_init(&peer.p_endpoint_mtx, IPL_NET);
    peer.p_endpoint.set(WgEndpoint::default());

    let arg = ptr::from_ref(peer).cast_mut().cast::<c_void>();
    task_set(&peer.p_send_initiation, wg_send_initiation, arg);
    task_set(&peer.p_send_keepalive, wg_send_keepalive, arg);
    task_set(&peer.p_clear_secrets, wg_peer_clear_secrets, arg);
    task_set(&peer.p_deliver_out, wg_deliver_out, arg);
    task_set(&peer.p_deliver_in, wg_deliver_in, arg);

    mq_init(&peer.p_stage_queue, MAX_STAGED_PKT, IPL_NET);
    mtx_init(&peer.p_encap_queue.q_mtx, IPL_NET);
    ml_init(&peer.p_encap_queue.q_list);
    mtx_init(&peer.p_decap_queue.q_mtx, IPL_NET);
    ml_init(&peer.p_decap_queue.q_list);

    peer.p_unused_index.init();
    for index in &peer.p_index {
        // SAFETY: the list was just emptied and each index is inserted once.
        unsafe { peer.p_unused_index.insert_head(index) };
    }

    peer.p_aip.init();

    peer.p_start_onlist.set(false);

    let idx = SipHash24(&sc.sc_secret.get(), public) & sc.sc_peer_mask.get();

    rw_enter_write(&sc.sc_peer_lock);
    if let Some(bucket) = sc.sc_peer.get().and_then(|t| t.get(idx as usize)) {
        // SAFETY: a new peer, on no list; `sc_peer_lock` is held.
        unsafe { bucket.insert_head(peer) };
    }
    // SAFETY: as above.
    unsafe { sc.sc_peer_seq.insert_tail(peer) };
    sc.sc_peer_num.set(sc.sc_peer_num.get() + 1);
    rw_exit_write(&sc.sc_peer_lock);

    wgprintf!(LOG_INFO, sc, None, "Peer {} created\n", peer.p_id.get());
    Some(peer)
}

/// `wg_peer_lookup`: the peer whose static public key is `public`.
pub fn wg_peer_lookup(sc: &WgSoftc, public: &[u8; WG_KEY_SIZE]) -> Option<&'static WgPeer> {
    let mut peer_key = [0u8; WG_KEY_SIZE];

    let idx = SipHash24(&sc.sc_secret.get(), public) & sc.sc_peer_mask.get();

    rw_enter_read(&sc.sc_peer_lock);
    let found = sc
        .sc_peer
        .get()
        .and_then(|t| t.get(idx as usize))
        .and_then(|bucket| {
            bucket.iter().find(|peer| {
                let _ = noise_remote_keys(&peer.p_remote, Some(&mut peer_key), None);
                !timingsafe_bcmp(&peer_key, public)
            })
        });
    rw_exit_read(&sc.sc_peer_lock);
    found
}

/// `wg_peer_destroy`: unlinks the peer, stops its timers, drops its allowed IPs and indices,
/// waits for the queues that may still hold it and frees it. Called with `sc_lock` held for
/// writing.
pub fn wg_peer_destroy(peer: &'static WgPeer) {
    let sc = peer.sc();

    rw_assert_wrlock(&sc.sc_lock);

    // Remove peer from the pubkey hashtable and disable all timeouts. After this, and
    // flushing wg_handshake_taskq, then no more handshakes can be started.
    rw_enter_write(&sc.sc_peer_lock);
    // SAFETY: the peer is on its bucket and on `sc_peer_seq` (`wg_peer_create`);
    // `sc_peer_lock` is held.
    unsafe {
        ListHead::<WgPeersByKey>::remove(peer);
        sc.sc_peer_seq.remove(peer);
    }
    sc.sc_peer_num.set(sc.sc_peer_num.get() - 1);
    rw_exit_write(&sc.sc_peer_lock);

    wg_timers_disable(&peer.p_timers);

    taskq_barrier(wg_handshake_taskq());

    // Now we drop all allowed ips, to drop all outgoing packets to the peer. Then drop all
    // the indexes to drop all incoming packets to the peer. Then we can flush if_snd,
    // wg_crypt_taskq and then nettq to ensure no more references to the peer exist.
    for aip in peer.p_aip.iter() {
        let _ = wg_aip_remove(sc, peer, &aip.a_data.get());
    }

    noise_remote_clear(&peer.p_remote);

    net_lock();
    while !ifq_empty(&sc.sc_if.if_snd) {
        // XXX: `if_snd' of stopped interface could still contain packets
        if sc.sc_if.if_flags.get() & IFF_RUNNING == 0 {
            ifq_purge(&sc.sc_if.if_snd);
            continue;
        }
        net_unlock();
        let _ = tsleep_nsec(ptr::from_ref(&NOWAKE), PWAIT, "wg_ifq", 1000);
        net_lock();
    }
    net_unlock();

    taskq_barrier(wg_crypt_taskq());
    if let Some(tq) = net_tq(sc.sc_if.if_index.get()) {
        taskq_barrier(tq);
    }

    if !mq_empty(&peer.p_stage_queue) {
        mq_purge(&peer.p_stage_queue);
    }

    wgprintf!(LOG_INFO, sc, None, "Peer {} destroyed\n", peer.p_id.get());
    // explicit_bzero(peer, sizeof(*peer)): the secrets (see the module's deviations).
    peer.p_remote.r_public.set([0; NOISE_PUBLIC_KEY_LEN]);
    peer.p_remote.r_ss.set([0; NOISE_PUBLIC_KEY_LEN]);
    peer.p_remote.r_psk.set([0; WG_KEY_SIZE]);
    peer.p_remote.r_handshake.set(NoiseHandshake::default());
    peer.p_remote.r_timestamp.set([0; NOISE_TIMESTAMP_LEN]);
    peer.p_cookie.cp_mac1_key.set([0; 32]);
    peer.p_cookie.cp_cookie_key.set([0; 32]);
    peer.p_cookie.cp_cookie.set([0; 16]);
    peer.p_cookie.cp_mac1_last.set([0; 16]);
    peer.p_endpoint.set(WgEndpoint::default());
    pool_put(&WG_PEER_POOL, NonNull::from(peer).cast());
}

/// `wg_peer_set_endpoint_from_tag`: the peer's endpoint becomes where the packet came from.
pub fn wg_peer_set_endpoint_from_tag(peer: &WgPeer, t: &WgTag) {
    if t.t_endpoint.get() == peer.p_endpoint.get() {
        return;
    }

    mtx_enter(&peer.p_endpoint_mtx);
    peer.p_endpoint.set(t.t_endpoint.get());
    mtx_leave(&peer.p_endpoint_mtx);
}

/// `wg_peer_set_sockaddr`: the peer's remote address, and no local one.
pub fn wg_peer_set_sockaddr(peer: &WgPeer, remote: &WgPeerEndpoint) {
    mtx_enter(&peer.p_endpoint_mtx);
    let mut e = peer.p_endpoint.get();
    e.e_remote = *remote;
    e.e_local = WgLocal::default();
    peer.p_endpoint.set(e);
    mtx_leave(&peer.p_endpoint_mtx);
}

/// `wg_peer_get_sockaddr`: the peer's remote address; `ENOENT` when it has none.
pub fn wg_peer_get_sockaddr(peer: &WgPeer, remote: &mut WgPeerEndpoint) -> Result<(), Errno> {
    let mut ret = Ok(());

    mtx_enter(&peer.p_endpoint_mtx);
    let e = peer.p_endpoint.get();
    if e.e_remote.sa_family() != AF_UNSPEC {
        *remote = e.e_remote;
    } else {
        ret = Err(Errno::ENOENT);
    }
    mtx_leave(&peer.p_endpoint_mtx);
    ret
}

/// `wg_peer_clear_src`: forgets the local address to send from.
pub fn wg_peer_clear_src(peer: &WgPeer) {
    mtx_enter(&peer.p_endpoint_mtx);
    let mut e = peer.p_endpoint.get();
    e.e_local = WgLocal::default();
    peer.p_endpoint.set(e);
    mtx_leave(&peer.p_endpoint_mtx);
}

/// `wg_peer_get_endpoint`.
pub fn wg_peer_get_endpoint(peer: &WgPeer) -> WgEndpoint {
    mtx_enter(&peer.p_endpoint_mtx);
    let e = peer.p_endpoint.get();
    mtx_leave(&peer.p_endpoint_mtx);
    e
}

/// `wg_peer_counters_add`.
pub fn wg_peer_counters_add(peer: &WgPeer, tx: u64, rx: u64) {
    mtx_enter(&peer.p_counters_mtx);
    peer.p_counters_tx.set(peer.p_counters_tx.get() + tx);
    peer.p_counters_rx.set(peer.p_counters_rx.get() + rx);
    mtx_leave(&peer.p_counters_mtx);
}

/// `wg_aip_add`: routes the prefix `d` to `peer` (taking it from another peer if it had it).
pub fn wg_aip_add(sc: &WgSoftc, peer: &'static WgPeer, d: &WgAipIo) -> Result<(), Errno> {
    let root = match d.a_af {
        AF_INET => sc.aip4(),
        #[cfg(feature = "inet6")]
        AF_INET6 => sc.aip6(),
        _ => return Err(Errno::EAFNOSUPPORT),
    };

    if d.a_cidr < 0 || d.a_cidr as u32 > root.art_alen.get() {
        return Err(Errno::EINVAL);
    }

    let Some(p) = pool_get(&WG_AIP_POOL, PR_NOWAIT | PR_ZERO) else {
        return Err(Errno::ENOBUFS);
    };
    let p = p.cast::<WgAip>();
    // SAFETY: a zero-filled pool item of `size_of::<WgAip>()` bytes, aligned for it; all zero
    // is a valid `WgAip`. It lives until `wg_aip_remove` gives it back.
    let aip: &'static WgAip = unsafe { p.as_ref() };

    art_node_init(&aip.a_node, &d.a_addr.addr_bytes, d.a_cidr as u32);

    let mut ret = Ok(());
    rw_enter_write(&sc.sc_aip_lock);
    match art_insert(root, &aip.a_node) {
        Some(node) if ptr::eq(node, &aip.a_node) => {
            aip.a_peer.set(Some(peer));
            aip.a_data.set(*d);
            // SAFETY: a new allowed IP, on no list; `sc_aip_lock` is held.
            unsafe { peer.p_aip.insert_head(aip) };
            sc.sc_aip_num.set(sc.sc_aip_num.get() + 1);
        }
        Some(node) => {
            pool_put(&WG_AIP_POOL, p.cast());
            let aip = WgAip::of_node(node);
            if !aip.a_peer.get().is_some_and(|p| ptr::eq(p, peer)) {
                // SAFETY: the allowed IP is on its peer's list; `sc_aip_lock` is held.
                unsafe {
                    ListHead::<WgAips>::remove(aip);
                    peer.p_aip.insert_head(aip);
                }
                aip.a_peer.set(Some(peer));
            }
        }
        None => {
            pool_put(&WG_AIP_POOL, p.cast());
            ret = Err(Errno::ENOBUFS);
        }
    }
    rw_exit_write(&sc.sc_aip_lock);
    ret
}

/// `wg_aip_lookup`: the peer whose allowed IPs best match `addr`.
pub fn wg_aip_lookup(root: &Art, addr: &[u8]) -> Option<&'static WgPeer> {
    // smr_read_enter()/smr_read_leave(): SMR is not ported (`net/art.rs`).
    let node = art_match(root, addr);

    node.and_then(|node| WgAip::of_node(node).a_peer.get())
}

/// `wg_aip_remove`: removes the prefix `d` of `peer`; `ENOENT` when there is none, `EXDEV`
/// when it belongs to another peer.
pub fn wg_aip_remove(sc: &WgSoftc, peer: &WgPeer, d: &WgAipIo) -> Result<(), Errno> {
    let root = match d.a_af {
        AF_INET => sc.aip4(),
        #[cfg(feature = "inet6")]
        AF_INET6 => sc.aip6(),
        _ => return Err(Errno::EAFNOSUPPORT),
    };
    if d.a_cidr < 0 || d.a_cidr as u32 > root.art_alen.get() {
        return Err(Errno::ENOENT);
    }
    let plen = d.a_cidr as u32;

    rw_enter_write(&sc.sc_aip_lock);
    let node = art_lookup(root, &d.a_addr.addr_bytes, plen);
    let ret = match node {
        None => Err(Errno::ENOENT),
        Some(node)
            if !WgAip::of_node(node)
                .a_peer
                .get()
                .is_some_and(|p| ptr::eq(p, peer)) =>
        {
            Err(Errno::EXDEV)
        }
        Some(node) => {
            let aip = WgAip::of_node(node);
            if art_delete(root, &d.a_addr.addr_bytes, plen).is_none() {
                panic(format_args!("art_delete failed to delete node {:p}", node));
            }

            sc.sc_aip_num.set(sc.sc_aip_num.get() - 1);
            // SAFETY: the allowed IP is on its peer's list; `sc_aip_lock` is held.
            unsafe { ListHead::<WgAips>::remove(aip) };
            pool_put(&WG_AIP_POOL, NonNull::from(aip).cast());
            Ok(())
        }
    };
    rw_exit_write(&sc.sc_aip_lock);
    ret
}

/// A fresh mbuf for an address or option argument (the C's stack `struct mbuf`).
fn m_arg(type_: i32) -> Result<&'static Mbuf, Errno> {
    m_get(M_WAIT, type_).ok_or(Errno::ENOBUFS)
}

/// `wg_socket_open`: a UDP socket of `af` bound to `port` in routing table `rtable`, whose
/// datagrams go to `wg_input`; `port` and `rtable` get what the socket was bound to.
pub fn wg_socket_open(
    so: &Cell<Option<&'static Socket>>,
    af: SaFamily,
    port: &mut InPort,
    rtable: &mut i32,
    upcall_arg: *mut c_void,
) -> Result<(), Errno> {
    let mhostnam = m_arg(MT_SONAME)?;
    let mrtable = match m_arg(MT_SOOPTS) {
        Ok(m) => m,
        Err(e) => {
            m_free(mhostnam);
            return Err(e);
        }
    };

    // SAFETY: a fresh mbuf's data area holds far more than an `int`.
    unsafe { mtod::<u32>(mrtable).write_unaligned(*rtable as u32) };
    mrtable.m_len().set(size_of::<u32>() as u32);

    if af == AF_INET {
        let sin = SockaddrIn {
            sin_len: size_of::<SockaddrIn>() as u8,
            sin_family: AF_INET,
            sin_port: *port,
            sin_addr: InAddr { s_addr: INADDR_ANY },
            ..SockaddrIn::default()
        };
        // SAFETY: as above, for a `sockaddr_in`.
        unsafe { mtod::<SockaddrIn>(mhostnam).write_unaligned(sin) };
        mhostnam.m_len().set(u32::from(sin.sin_len));
    } else if wg_hostnam_in6(af, mhostnam, *port) {
        // The sockaddr_in6 of in6addr_any is in `mhostnam`.
    } else {
        m_free(mhostnam);
        m_free(mrtable);
        return Err(Errno::EAFNOSUPPORT);
    }

    let ret = 'out: {
        let s = match socreate(i32::from(af), SOCK_DGRAM, 0) {
            Ok(s) => s,
            Err(e) => break 'out Err(e),
        };
        so.set(Some(s));

        solock(s);
        if let Some(inp) = sotoinpcb(s) {
            inp.inp_upcall.set(Some(wg_input));
            inp.inp_upcall_arg.set(upcall_arg);
        }
        let mut ret = Ok(());
        sounlock(s);

        if ret.is_ok() {
            ret = sosetopt(s, SOL_SOCKET, SO_RTABLE, Some(mrtable));
        }
        if ret.is_ok() {
            solock(s);
            let p = match curproc() {
                Some(p) => p,
                None => panic(format_args!("wg_socket_open: no curproc")),
            };
            ret = sobind(s, mhostnam, p);
            if ret.is_ok()
                && let Some(inp) = sotoinpcb(s)
            {
                *port = inp.inp_lport.get();
                *rtable = inp.inp_rtableid.get() as i32;
            }
            sounlock(s);
        }
        ret
    };

    m_free(mhostnam);
    m_free(mrtable);

    if ret.is_err() {
        wg_socket_close(so);
    }

    ret
}

/// The `AF_INET6` half of `wg_socket_open`'s address: a `sockaddr_in6` of `in6addr_any` and
/// `port` in `mhostnam`; `false` when `af` is not `AF_INET6` (always, without `INET6`).
#[cfg(feature = "inet6")]
fn wg_hostnam_in6(af: SaFamily, mhostnam: &'static Mbuf, port: InPort) -> bool {
    if af != AF_INET6 {
        return false;
    }
    let sin6 = SockaddrIn6 {
        sin6_len: size_of::<SockaddrIn6>() as u8,
        sin6_family: AF_INET6,
        sin6_port: port,
        sin6_addr: IN6ADDR_ANY,
        ..SockaddrIn6::default()
    };
    // SAFETY: a fresh mbuf's data area holds far more than a `sockaddr_in6`.
    unsafe { mtod::<SockaddrIn6>(mhostnam).write_unaligned(sin6) };
    mhostnam.m_len().set(u32::from(sin6.sin6_len));
    true
}

/// The `AF_INET6` half of `wg_socket_open`'s address (without `INET6`: never).
#[cfg(not(feature = "inet6"))]
fn wg_hostnam_in6(_af: SaFamily, _mhostnam: &'static Mbuf, _port: InPort) -> bool {
    false
}

/// `wg_socket_close`.
pub fn wg_socket_close(so: &Cell<Option<&'static Socket>>) {
    if let Some(s) = so.get()
        && soclose(s, 0).is_err()
    {
        panic(format_args!("Unable to close wg socket"));
    }
    so.set(None);
}

/// `wg_bind`: opens the interface's sockets on `*portp` in `*rtablep` and replaces the old
/// ones; the port and table actually bound go back through the pointers.
pub fn wg_bind(sc: &WgSoftc, portp: &mut InPort, rtablep: &mut i32) -> Result<(), Errno> {
    let so4: Cell<Option<&'static Socket>> = Cell::new(None);
    #[cfg(feature = "inet6")]
    let so6: Cell<Option<&'static Socket>> = Cell::new(None);
    #[cfg(feature = "inet6")]
    let mut retries = 0;

    let mut port;
    let mut rtable;
    let arg = ptr::from_ref(sc).cast_mut().cast::<c_void>();
    loop {
        // retry:
        port = *portp;
        rtable = *rtablep;
        wg_socket_open(&so4, AF_INET, &mut port, &mut rtable, arg)?;

        #[cfg(feature = "inet6")]
        if let Err(ret) = wg_socket_open(&so6, AF_INET6, &mut port, &mut rtable, arg) {
            wg_socket_close(&so4);
            if ret == Errno::EADDRINUSE && *portp == 0 && retries < 100 {
                retries += 1;
                continue;
            }
            return Err(ret);
        }
        break;
    }

    rw_enter_write(&sc.sc_so_lock);
    wg_socket_close(&sc.sc_so4);
    sc.sc_so4.set(so4.get());
    #[cfg(feature = "inet6")]
    {
        wg_socket_close(&sc.sc_so6);
        sc.sc_so6.set(so6.get());
    }
    rw_exit_write(&sc.sc_so_lock);

    *portp = port;
    *rtablep = rtable;
    Ok(())
}

/// `wg_unbind`.
pub fn wg_unbind(sc: &WgSoftc) {
    rw_enter_write(&sc.sc_so_lock);
    wg_socket_close(&sc.sc_so4);
    #[cfg(feature = "inet6")]
    wg_socket_close(&sc.sc_so6);
    rw_exit_write(&sc.sc_so_lock);
}

/// `wg_send`: sends `m` to the endpoint (from its local address, if it has one) through the
/// interface's socket of its family. `m` is consumed.
pub fn wg_send(sc: &WgSoftc, e: &WgEndpoint, m: &'static Mbuf) -> Result<(), Errno> {
    let mut control: Option<&'static Mbuf> = None;

    // Get local control address before locking
    let family = e.e_remote.sa_family();
    match family {
        AF_INET => {
            if e.e_local.l_in().s_addr != INADDR_ANY {
                control = sbcreatecontrol(&in_bytes(&e.e_local.l_in()), IP_SENDSRCADDR, IPPROTO_IP);
            }
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            if !in6_is_addr_unspecified(&e.e_local.l_in6()) {
                control = sbcreatecontrol(e.e_local.as_bytes(), IPV6_PKTINFO, IPPROTO_IPV6);
            }
        }
        _ => {
            m_freem(m);
            return Err(Errno::EAFNOSUPPORT);
        }
    }

    // Get remote address
    let Some(peernam) = m_get(M_WAIT, MT_SONAME) else {
        m_freem(control);
        m_freem(m);
        return Err(Errno::ENOBUFS);
    };
    // SAFETY: a fresh mbuf's data area holds far more than a `sockaddr_in6`.
    unsafe { mtod::<WgPeerEndpoint>(peernam).write_unaligned(e.e_remote) };
    peernam
        .m_len()
        .set(u32::from(e.e_remote.sa_len()).min(size_of::<WgPeerEndpoint>() as u32));

    rw_enter_read(&sc.sc_so_lock);
    let so = match family {
        AF_INET => sc.sc_so4.get(),
        #[cfg(feature = "inet6")]
        AF_INET6 => sc.sc_so6.get(),
        _ => None,
    };
    let ret = match so {
        Some(so) => sosend(so, Some(peernam), None, Some(m), control, 0),
        None => {
            m_freem(control);
            m_freem(m);
            Err(Errno::ENOTCONN)
        }
    };
    rw_exit_read(&sc.sc_so_lock);

    m_free(peernam);
    ret
}

/// `wg_send_buf`: sends the handshake message `buf` to the endpoint, at the highest
/// priority; once more without the local address if it cannot be used.
pub fn wg_send_buf(sc: &WgSoftc, e: &mut WgEndpoint, buf: &[u8]) {
    let len = buf.len();
    let mut ret: Result<(), Errno> = Ok(());
    let mlen = len + MAX_HDR.load(Ordering::Relaxed) as usize;

    loop {
        // retry:
        let Some(m) = m_gethdr(M_WAIT, MT_DATA) else {
            return;
        };
        if mlen > MHLEN {
            let _ = m_clget(Some(m), M_WAIT, mlen as u32);
            if m.m_flags().get() & M_EXT == 0 {
                m_freem(m);
                return;
            }
        }
        m_align(m, len as i32);
        m.m_pkthdr().len.set(len as i32);
        m.m_len().set(len as u32);
        // SAFETY: the mbuf's data area holds `len` bytes after `m_align`; `buf` is another
        // object.
        unsafe { ptr::copy_nonoverlapping(buf.as_ptr(), mtod::<u8>(m), len) };

        // As we're sending a handshake packet here, we want high priority
        m.m_pkthdr().pf.prio.set(IFQ_MAXPRIO as u8);

        if ret.is_ok() {
            ret = wg_send(sc, e, m);
            // Retry if we couldn't bind to e->e_local
            if ret == Err(Errno::EADDRNOTAVAIL) {
                e.e_local = WgLocal::default();
                continue;
            }
        } else {
            ret = wg_send(sc, e, m);
            if ret.is_err() {
                wgprintf!(LOG_DEBUG, sc, None, "Unable to send packet\n");
            }
        }
        return;
    }
}

/// `wg_tag_get`: the packet's WireGuard tag, attached zeroed if it had none.
pub fn wg_tag_get(m: &'static Mbuf) -> Option<&'static WgTag> {
    let mtag: &'static MTag = match m_tag_find(m, PACKET_TAG_WIREGUARD, None) {
        Some(t) => t,
        None => {
            let t = m_tag_get(PACKET_TAG_WIREGUARD, size_of::<WgTag>() as i32, M_DONTWAIT)?;
            // SAFETY: the tag's data area holds `size_of::<WgTag>()` bytes (asked for above),
            // not yet referenced.
            unsafe { ptr::write_bytes(t.data(), 0, size_of::<WgTag>()) };
            m_tag_prepend(m, t);
            t
        }
    };
    // SAFETY: a `PACKET_TAG_WIREGUARD` tag's data is a `WgTag` (zeroed above, all zero valid),
    // aligned (the data follows the 8-aligned `MTag`, asserted at the end), and lives as long as
    // the packet.
    Some(unsafe { &*mtag.data().cast::<WgTag>() })
}

// The following section handles the timeout callbacks for a WireGuard session. These
// functions provide an "event based" model for controlling wg(8) session timers. All function
// calls occur after the specified event below.
//
// wg_timers_event_data_sent:
//	tx: data
// wg_timers_event_data_received:
//	rx: data
// wg_timers_event_any_authenticated_packet_sent:
//	tx: keepalive, data, handshake
// wg_timers_event_any_authenticated_packet_received:
//	rx: keepalive, data, handshake
// wg_timers_event_any_authenticated_packet_traversal:
//	tx, rx: keepalive, data, handshake
// wg_timers_event_handshake_initiated:
//	tx: initiation
// wg_timers_event_handshake_responded:
//	tx: response
// wg_timers_event_handshake_complete:
//	rx: response, confirmation data
// wg_timers_event_session_derived:
//	tx: response, rx: response
// wg_timers_event_want_initiation:
//	tx: data failed, old keys expiring
// wg_timers_event_reset_handshake_last_sent:
// 	anytime we may immediately want a new handshake

/// `wg_timers_init`.
pub fn wg_timers_init(t: &WgTimers) {
    // bzero(t, sizeof(*t))
    t.t_disabled.set(false);
    t.t_need_another_keepalive.set(false);
    t.t_persistent_keepalive_interval.set(0);
    t.t_handshake_last_sent.set(Timespec::default());
    t.t_handshake_complete.set(Timespec::default());
    t.t_handshake_retries.set(0);
    mtx_init_flags(&t.t_mtx, IPL_NET, Some("wg_timers"), 0);
    mtx_init(&t.t_handshake_mtx, IPL_NET);

    let arg = ptr::from_ref(t).cast_mut().cast::<c_void>();
    timeout_set(&t.t_new_handshake, wg_timers_run_new_handshake, arg);
    timeout_set(&t.t_send_keepalive, wg_timers_run_send_keepalive, arg);
    timeout_set(&t.t_retry_handshake, wg_timers_run_retry_handshake, arg);
    timeout_set(
        &t.t_persistent_keepalive,
        wg_timers_run_persistent_keepalive,
        arg,
    );
    timeout_set(&t.t_zero_key_material, wg_timers_run_zero_key_material, arg);
}

/// `wg_timers_enable`.
pub fn wg_timers_enable(t: &WgTimers) {
    mtx_enter(&t.t_mtx);
    t.t_disabled.set(false);
    mtx_leave(&t.t_mtx);
    wg_timers_run_persistent_keepalive(ptr::from_ref(t).cast_mut().cast());
}

/// `wg_timers_disable`.
pub fn wg_timers_disable(t: &WgTimers) {
    mtx_enter(&t.t_mtx);
    t.t_disabled.set(true);
    t.t_need_another_keepalive.set(false);
    mtx_leave(&t.t_mtx);

    timeout_del_barrier(&t.t_new_handshake);
    timeout_del_barrier(&t.t_send_keepalive);
    timeout_del_barrier(&t.t_retry_handshake);
    timeout_del_barrier(&t.t_persistent_keepalive);
    timeout_del_barrier(&t.t_zero_key_material);
}

/// `wg_timers_set_persistent_keepalive`.
pub fn wg_timers_set_persistent_keepalive(t: &WgTimers, interval: u16) {
    mtx_enter(&t.t_mtx);
    if !t.t_disabled.get() {
        t.t_persistent_keepalive_interval.set(interval);
        wg_timers_run_persistent_keepalive(ptr::from_ref(t).cast_mut().cast());
    }
    mtx_leave(&t.t_mtx);
}

/// `wg_timers_get_persistent_keepalive`: `ENOENT` when there is none.
pub fn wg_timers_get_persistent_keepalive(t: &WgTimers, interval: &mut u16) -> Result<(), Errno> {
    *interval = t.t_persistent_keepalive_interval.get();
    if *interval > 0 {
        Ok(())
    } else {
        Err(Errno::ENOENT)
    }
}

/// `wg_timers_get_last_handshake`.
pub fn wg_timers_get_last_handshake(t: &WgTimers) -> Timespec {
    mtx_enter(&t.t_handshake_mtx);
    let time = t.t_handshake_complete.get();
    mtx_leave(&t.t_handshake_mtx);
    time
}

/// `wg_timers_expired_handshake_last_sent`: was the last handshake sent more than
/// `REKEY_TIMEOUT` seconds ago?
pub fn wg_timers_expired_handshake_last_sent(t: &WgTimers) -> bool {
    let expire = Timespec::new(i64::from(REKEY_TIMEOUT), 0);

    let uptime = getnanouptime();
    let expire = timespecadd(&t.t_handshake_last_sent.get(), &expire);
    uptime > expire
}

/// `wg_timers_check_handshake_last_sent`: as the above, and when it was, a handshake is sent
/// now.
pub fn wg_timers_check_handshake_last_sent(t: &WgTimers) -> bool {
    mtx_enter(&t.t_handshake_mtx);
    let ret = wg_timers_expired_handshake_last_sent(t);
    if ret {
        t.t_handshake_last_sent.set(getnanouptime());
    }
    mtx_leave(&t.t_handshake_mtx);
    ret
}

/// `wg_timers_event_data_sent`.
pub fn wg_timers_event_data_sent(t: &WgTimers) {
    let mut msecs = (NEW_HANDSHAKE_TIMEOUT * 1000) as u64;
    msecs += u64::from(arc4random_uniform(REKEY_TIMEOUT_JITTER));

    mtx_enter(&t.t_mtx);
    if !t.t_disabled.get() && !timeout_pending(&t.t_new_handshake) {
        timeout_add_msec(&t.t_new_handshake, msecs);
    }
    mtx_leave(&t.t_mtx);
}

/// `wg_timers_event_data_received`.
pub fn wg_timers_event_data_received(t: &WgTimers) {
    mtx_enter(&t.t_mtx);
    if !t.t_disabled.get() {
        if !timeout_pending(&t.t_send_keepalive) {
            timeout_add_sec(&t.t_send_keepalive, KEEPALIVE_TIMEOUT);
        } else {
            t.t_need_another_keepalive.set(true);
        }
    }
    mtx_leave(&t.t_mtx);
}

/// `wg_timers_event_any_authenticated_packet_sent`.
pub fn wg_timers_event_any_authenticated_packet_sent(t: &WgTimers) {
    timeout_del(&t.t_send_keepalive);
}

/// `wg_timers_event_any_authenticated_packet_received`.
pub fn wg_timers_event_any_authenticated_packet_received(t: &WgTimers) {
    timeout_del(&t.t_new_handshake);
}

/// `wg_timers_event_any_authenticated_packet_traversal`.
pub fn wg_timers_event_any_authenticated_packet_traversal(t: &WgTimers) {
    mtx_enter(&t.t_mtx);
    if !t.t_disabled.get() && t.t_persistent_keepalive_interval.get() > 0 {
        timeout_add_sec(
            &t.t_persistent_keepalive,
            i32::from(t.t_persistent_keepalive_interval.get()),
        );
    }
    mtx_leave(&t.t_mtx);
}

/// `wg_timers_event_handshake_initiated`.
pub fn wg_timers_event_handshake_initiated(t: &WgTimers) {
    let mut msecs = (REKEY_TIMEOUT * 1000) as u64;
    msecs += u64::from(arc4random_uniform(REKEY_TIMEOUT_JITTER));

    mtx_enter(&t.t_mtx);
    if !t.t_disabled.get() {
        timeout_add_msec(&t.t_retry_handshake, msecs);
    }
    mtx_leave(&t.t_mtx);
}

/// `wg_timers_event_handshake_responded`.
pub fn wg_timers_event_handshake_responded(t: &WgTimers) {
    mtx_enter(&t.t_handshake_mtx);
    t.t_handshake_last_sent.set(getnanouptime());
    mtx_leave(&t.t_handshake_mtx);
}

/// `wg_timers_event_handshake_complete`.
pub fn wg_timers_event_handshake_complete(t: &WgTimers) {
    mtx_enter(&t.t_mtx);
    if !t.t_disabled.get() {
        mtx_enter(&t.t_handshake_mtx);
        timeout_del(&t.t_retry_handshake);
        t.t_handshake_retries.set(0);
        t.t_handshake_complete.set(getnanotime());
        mtx_leave(&t.t_handshake_mtx);
        wg_timers_run_send_keepalive(ptr::from_ref(t).cast_mut().cast());
    }
    mtx_leave(&t.t_mtx);
}

/// `wg_timers_event_session_derived`.
pub fn wg_timers_event_session_derived(t: &WgTimers) {
    mtx_enter(&t.t_mtx);
    if !t.t_disabled.get() {
        timeout_add_sec(&t.t_zero_key_material, (REJECT_AFTER_TIME * 3) as i32);
    }
    mtx_leave(&t.t_mtx);
}

/// `wg_timers_event_want_initiation`.
pub fn wg_timers_event_want_initiation(t: &WgTimers) {
    mtx_enter(&t.t_mtx);
    if !t.t_disabled.get() {
        wg_timers_run_send_initiation(t, false);
    }
    mtx_leave(&t.t_mtx);
}

/// `wg_timers_event_reset_handshake_last_sent`.
pub fn wg_timers_event_reset_handshake_last_sent(t: &WgTimers) {
    mtx_enter(&t.t_handshake_mtx);
    let mut last = t.t_handshake_last_sent.get();
    last.tv_sec -= i64::from(REKEY_TIMEOUT + 1);
    t.t_handshake_last_sent.set(last);
    mtx_leave(&t.t_handshake_mtx);
}

/// The `WgTimers` a timeout was set with.
fn timers_of(arg: *mut c_void) -> &'static WgTimers {
    // SAFETY: `wg_timers_init` sets every timeout with its `WgTimers` as the argument, and the
    // timeouts are deleted with a barrier (`wg_timers_disable`) before the peer is freed.
    unsafe { &*arg.cast::<WgTimers>() }
}

/// `wg_timers_run_send_initiation`.
pub fn wg_timers_run_send_initiation(t: &WgTimers, is_retry: bool) {
    let peer = WgPeer::of_timers(t);
    if !is_retry {
        t.t_handshake_retries.set(0);
    }
    if wg_timers_expired_handshake_last_sent(t) {
        task_add(wg_handshake_taskq(), &peer.p_send_initiation);
    }
}

/// `wg_timers_run_retry_handshake`.
pub fn wg_timers_run_retry_handshake(arg: *mut c_void) {
    let t = timers_of(arg);
    let peer = WgPeer::of_timers(t);

    mtx_enter(&t.t_handshake_mtx);
    if t.t_handshake_retries.get() <= MAX_TIMER_HANDSHAKES {
        t.t_handshake_retries.set(t.t_handshake_retries.get() + 1);
        mtx_leave(&t.t_handshake_mtx);

        wgprintf!(
            LOG_INFO,
            peer.sc(),
            Some(&peer.p_endpoint_mtx),
            "Handshake for peer {} ({}) did not complete after {} seconds, retrying (try {})\n",
            peer.p_id.get(),
            SaNtop::of(&peer.p_endpoint.get().e_remote),
            REKEY_TIMEOUT,
            t.t_handshake_retries.get() + 1
        );
        wg_peer_clear_src(peer);
        wg_timers_run_send_initiation(t, true);
    } else {
        mtx_leave(&t.t_handshake_mtx);

        wgprintf!(
            LOG_INFO,
            peer.sc(),
            Some(&peer.p_endpoint_mtx),
            "Handshake for peer {} ({}) did not complete after {} retries, giving up\n",
            peer.p_id.get(),
            SaNtop::of(&peer.p_endpoint.get().e_remote),
            MAX_TIMER_HANDSHAKES + 2
        );

        timeout_del(&t.t_send_keepalive);
        mq_purge(&peer.p_stage_queue);
        if !timeout_pending(&t.t_zero_key_material) {
            timeout_add_sec(&t.t_zero_key_material, (REJECT_AFTER_TIME * 3) as i32);
        }
    }
}

/// `wg_timers_run_send_keepalive`.
pub fn wg_timers_run_send_keepalive(arg: *mut c_void) {
    let t = timers_of(arg);
    let peer = WgPeer::of_timers(t);

    task_add(wg_crypt_taskq(), &peer.p_send_keepalive);
    if t.t_need_another_keepalive.get() {
        t.t_need_another_keepalive.set(false);
        timeout_add_sec(&t.t_send_keepalive, KEEPALIVE_TIMEOUT);
    }
}

/// `wg_timers_run_new_handshake`.
pub fn wg_timers_run_new_handshake(arg: *mut c_void) {
    let t = timers_of(arg);
    let peer = WgPeer::of_timers(t);

    wgprintf!(
        LOG_INFO,
        peer.sc(),
        Some(&peer.p_endpoint_mtx),
        "Retrying handshake with peer {} ({}) because we stopped hearing back after {} seconds\n",
        peer.p_id.get(),
        SaNtop::of(&peer.p_endpoint.get().e_remote),
        NEW_HANDSHAKE_TIMEOUT
    );
    wg_peer_clear_src(peer);

    wg_timers_run_send_initiation(t, false);
}

/// `wg_timers_run_zero_key_material`.
pub fn wg_timers_run_zero_key_material(arg: *mut c_void) {
    let t = timers_of(arg);
    let peer = WgPeer::of_timers(t);

    wgprintf!(
        LOG_INFO,
        peer.sc(),
        Some(&peer.p_endpoint_mtx),
        "Zeroing out keys for peer {} ({})\n",
        peer.p_id.get(),
        SaNtop::of(&peer.p_endpoint.get().e_remote)
    );
    task_add(wg_handshake_taskq(), &peer.p_clear_secrets);
}

/// `wg_timers_run_persistent_keepalive`.
pub fn wg_timers_run_persistent_keepalive(arg: *mut c_void) {
    let t = timers_of(arg);
    let peer = WgPeer::of_timers(t);
    if t.t_persistent_keepalive_interval.get() != 0 {
        task_add(wg_crypt_taskq(), &peer.p_send_keepalive);
    }
}

// The following functions handle handshakes

/// `wg_peer_send_buf`: sends a handshake message to the peer.
pub fn wg_peer_send_buf(peer: &WgPeer, buf: &[u8]) {
    wg_peer_counters_add(peer, buf.len() as u64, 0);
    wg_timers_event_any_authenticated_packet_traversal(&peer.p_timers);
    wg_timers_event_any_authenticated_packet_sent(&peer.p_timers);
    let mut endpoint = wg_peer_get_endpoint(peer);
    wg_send_buf(peer.sc(), &mut endpoint, buf);
}

/// The peer a task was set with.
fn peer_of(arg: *mut c_void) -> &'static WgPeer {
    // SAFETY: `wg_peer_create` sets the peer's tasks with the peer as the argument, and
    // `wg_peer_destroy` waits for the task queues (barriers) before it frees the peer.
    unsafe { &*arg.cast::<WgPeer>() }
}

/// `wg_send_initiation`: the `p_send_initiation` task.
pub fn wg_send_initiation(arg: *mut c_void) {
    let peer = peer_of(arg);
    let mut pkt = WgPktInitiation::zeroed();

    if !wg_timers_check_handshake_last_sent(&peer.p_timers) {
        return;
    }

    wgprintf!(
        LOG_INFO,
        peer.sc(),
        Some(&peer.p_endpoint_mtx),
        "Sending handshake initiation to peer {} ({})\n",
        peer.p_id.get(),
        SaNtop::of(&peer.p_endpoint.get().e_remote)
    );

    if noise_create_initiation(
        &peer.p_remote,
        &mut pkt.s_idx,
        &mut pkt.ue,
        &mut pkt.es,
        &mut pkt.ets,
    )
    .is_err()
    {
        return;
    }
    pkt.t = WG_PKT_INITIATION;
    let mut m = CookieMacs::default();
    cookie_maker_mac(
        &peer.p_cookie,
        &mut m,
        &pkt_bytes(&pkt)[..size_of::<WgPktInitiation>() - size_of::<CookieMacs>()],
    );
    pkt.m = m;
    wg_peer_send_buf(peer, pkt_bytes(&pkt));
    wg_timers_event_handshake_initiated(&peer.p_timers);
}

/// `wg_send_response`.
pub fn wg_send_response(peer: &'static WgPeer) {
    let mut pkt = WgPktResponse::default();

    wgprintf!(
        LOG_INFO,
        peer.sc(),
        Some(&peer.p_endpoint_mtx),
        "Sending handshake response to peer {} ({})\n",
        peer.p_id.get(),
        SaNtop::of(&peer.p_endpoint.get().e_remote)
    );

    if noise_create_response(
        &peer.p_remote,
        &mut pkt.s_idx,
        &mut pkt.r_idx,
        &mut pkt.ue,
        &mut pkt.en,
    )
    .is_err()
    {
        return;
    }
    if noise_remote_begin_session(&peer.p_remote).is_err() {
        return;
    }
    wg_timers_event_session_derived(&peer.p_timers);
    pkt.t = WG_PKT_RESPONSE;
    let mut m = CookieMacs::default();
    cookie_maker_mac(
        &peer.p_cookie,
        &mut m,
        &pkt_bytes(&pkt)[..size_of::<WgPktResponse>() - size_of::<CookieMacs>()],
    );
    pkt.m = m;
    wg_timers_event_handshake_responded(&peer.p_timers);
    wg_peer_send_buf(peer, pkt_bytes(&pkt));
}

/// `wg_send_cookie`: answers a handshake message received under load with a cookie.
pub fn wg_send_cookie(sc: &WgSoftc, cm: &CookieMacs, idx: u32, e: &mut WgEndpoint) {
    let mut pkt = WgPktCookie::default();

    wgprintf!(
        LOG_DEBUG,
        sc,
        None,
        "Sending cookie response for denied handshake message\n"
    );

    pkt.t = WG_PKT_COOKIE;
    pkt.r_idx = idx;

    cookie_checker_create_payload(
        &sc.sc_cookie,
        cm,
        &mut pkt.nonce,
        &mut pkt.ec,
        &e.e_remote.as_storage(),
    );

    wg_send_buf(sc, e, pkt_bytes(&pkt));
}

/// `wg_send_keepalive`: the `p_send_keepalive` task: stages an empty packet (unless packets
/// wait already) and sends the staged packets, or asks for a handshake.
pub fn wg_send_keepalive(arg: *mut c_void) {
    let peer = peer_of(arg);
    let sc = peer.sc();

    'send: {
        if !mq_empty(&peer.p_stage_queue) {
            break 'send;
        }

        let Some(m) = m_gethdr(M_DONTWAIT, MT_DATA) else {
            return;
        };

        let Some(t) = wg_tag_get(m) else {
            m_freem(m);
            return;
        };

        t.t_peer.set(Some(peer));
        t.t_mbuf.set(None);
        t.t_done.set(false);
        t.t_mtu.set(0); // MTU == 0 OK for keepalive

        let _ = mq_push(&peer.p_stage_queue, m);
    }
    // send:
    if noise_remote_ready(&peer.p_remote).is_ok() {
        wg_queue_out(sc, peer);
        task_add(wg_crypt_taskq(), &sc.sc_encap);
    } else {
        wg_timers_event_want_initiation(&peer.p_timers);
    }
}

/// `wg_peer_clear_secrets`: the `p_clear_secrets` task.
pub fn wg_peer_clear_secrets(arg: *mut c_void) {
    let peer = peer_of(arg);
    noise_remote_clear(&peer.p_remote);
}

/// The wire structure at the start of a contiguous packet of at least its size.
///
/// # Safety
///
/// `m`'s first buffer holds at least `size_of::<T>()` bytes (`wg_input` checked the length
/// and made the packet contiguous).
unsafe fn pkt_read<T: WgPkt>(m: &Mbuf) -> T {
    // SAFETY: the caller's contract; any bytes are a `T` (`WgPkt`); unaligned read.
    unsafe { mtod::<T>(m).read_unaligned() }
}

/// `wg_handshake`: processes a handshake message from the handshake queue.
pub fn wg_handshake(sc: &'static WgSoftc, m: &'static Mbuf) {
    let mut underload = false;

    mtx_enter(&WG_LAST_UNDERLOAD_MTX);
    // SAFETY: `WG_LAST_UNDERLOAD_MTX` is held until the reference's last use below.
    let last_underload = unsafe { WG_LAST_UNDERLOAD.get_mut() };
    if mq_len(&sc.sc_handshake_queue) >= MAX_QUEUED_HANDSHAKES / 8 {
        *last_underload = getmicrouptime();
        underload = true;
    } else if last_underload.tv_sec != 0 {
        if !ratecheck(last_underload, &UNDERLOAD_INTERVAL) {
            underload = true;
        } else {
            *last_underload = Timeval::new(0, 0);
        }
    }
    mtx_leave(&WG_LAST_UNDERLOAD_MTX);

    let Some(t) = wg_tag_get(m) else {
        m_freem(m);
        return;
    };
    let from = SaNtop::of(&t.t_endpoint.get().e_remote);

    // SAFETY: `wg_input` queued only contiguous packets at least as long as their type word.
    let ty = unsafe { mtod::<u32>(m).read_unaligned() };
    let peer: Option<&'static WgPeer> = 'error: {
        match ty {
            WG_PKT_INITIATION => {
                // SAFETY: `wg_input` queued it as an initiation of exactly this size.
                let init: WgPktInitiation = unsafe { pkt_read(m) };

                let res = cookie_checker_validate_macs(
                    &sc.sc_cookie,
                    &init.m,
                    &pkt_bytes(&init)[..size_of::<WgPktInitiation>() - size_of::<CookieMacs>()],
                    underload,
                    &t.t_endpoint.get().e_remote.as_storage(),
                );

                match res {
                    Ok(()) => {}
                    Err(Errno::EINVAL) => {
                        wgprintf!(LOG_INFO, sc, None, "Invalid initiation MAC from {}\n", from);
                        break 'error None;
                    }
                    Err(Errno::ECONNREFUSED) => {
                        wgprintf!(LOG_DEBUG, sc, None, "Handshake ratelimited from {}\n", from);
                        break 'error None;
                    }
                    Err(Errno::EAGAIN) => {
                        let mut e = t.t_endpoint.get();
                        wg_send_cookie(sc, &init.m, init.s_idx, &mut e);
                        t.t_endpoint.set(e);
                        break 'error None;
                    }
                    Err(e) => panic(format_args!("unexpected response: {}", e as i32)),
                }

                let Ok(remote) = noise_consume_initiation(
                    &sc.sc_local,
                    init.s_idx,
                    &init.ue,
                    &init.es,
                    &init.ets,
                ) else {
                    wgprintf!(
                        LOG_INFO,
                        sc,
                        None,
                        "Invalid handshake initiation from {}\n",
                        from
                    );
                    break 'error None;
                };

                let peer = WgPeer::of_remote(remote);

                wgprintf!(
                    LOG_INFO,
                    sc,
                    None,
                    "Receiving handshake initiation from peer {} ({})\n",
                    peer.p_id.get(),
                    from
                );

                wg_peer_counters_add(peer, 0, size_of::<WgPktInitiation>() as u64);
                wg_peer_set_endpoint_from_tag(peer, t);
                wg_send_response(peer);
                Some(peer)
            }
            WG_PKT_RESPONSE => {
                // SAFETY: `wg_input` queued it as a response of exactly this size.
                let resp: WgPktResponse = unsafe { pkt_read(m) };

                let res = cookie_checker_validate_macs(
                    &sc.sc_cookie,
                    &resp.m,
                    &pkt_bytes(&resp)[..size_of::<WgPktResponse>() - size_of::<CookieMacs>()],
                    underload,
                    &t.t_endpoint.get().e_remote.as_storage(),
                );

                match res {
                    Ok(()) => {}
                    Err(Errno::EINVAL) => {
                        wgprintf!(LOG_INFO, sc, None, "Invalid response MAC from {}\n", from);
                        break 'error None;
                    }
                    Err(Errno::ECONNREFUSED) => {
                        wgprintf!(LOG_DEBUG, sc, None, "Handshake ratelimited from {}\n", from);
                        break 'error None;
                    }
                    Err(Errno::EAGAIN) => {
                        let mut e = t.t_endpoint.get();
                        wg_send_cookie(sc, &resp.m, resp.s_idx, &mut e);
                        t.t_endpoint.set(e);
                        break 'error None;
                    }
                    Err(e) => panic(format_args!("unexpected response: {}", e as i32)),
                }

                let Some(remote) = wg_index_get(sc, resp.r_idx) else {
                    wgprintf!(
                        LOG_INFO,
                        sc,
                        None,
                        "Unknown handshake response from {}\n",
                        from
                    );
                    break 'error None;
                };

                let peer = WgPeer::of_remote(remote);

                if noise_consume_response(remote, resp.s_idx, resp.r_idx, &resp.ue, &resp.en)
                    .is_err()
                {
                    wgprintf!(
                        LOG_INFO,
                        sc,
                        None,
                        "Invalid handshake response from {}\n",
                        from
                    );
                    break 'error None;
                }

                wgprintf!(
                    LOG_INFO,
                    sc,
                    None,
                    "Receiving handshake response from peer {} ({})\n",
                    peer.p_id.get(),
                    from
                );

                wg_peer_counters_add(peer, 0, size_of::<WgPktResponse>() as u64);
                wg_peer_set_endpoint_from_tag(peer, t);
                if noise_remote_begin_session(&peer.p_remote).is_ok() {
                    wg_timers_event_session_derived(&peer.p_timers);
                    wg_timers_event_handshake_complete(&peer.p_timers);
                }
                Some(peer)
            }
            WG_PKT_COOKIE => {
                // SAFETY: `wg_input` queued it as a cookie message of exactly this size.
                let cook: WgPktCookie = unsafe { pkt_read(m) };

                let Some(remote) = wg_index_get(sc, cook.r_idx) else {
                    wgprintf!(LOG_DEBUG, sc, None, "Unknown cookie index from {}\n", from);
                    break 'error None;
                };

                let peer = WgPeer::of_remote(remote);

                if cookie_maker_consume_payload(&peer.p_cookie, &cook.nonce, &cook.ec).is_err() {
                    wgprintf!(
                        LOG_DEBUG,
                        sc,
                        None,
                        "Could not decrypt cookie response from {}\n",
                        from
                    );
                    break 'error None;
                }

                wgprintf!(
                    LOG_DEBUG,
                    sc,
                    None,
                    "Receiving cookie response from {}\n",
                    from
                );
                break 'error None;
            }
            _ => panic(format_args!("invalid packet in handshake queue")),
        }
    };

    if let Some(peer) = peer {
        wg_timers_event_any_authenticated_packet_received(&peer.p_timers);
        wg_timers_event_any_authenticated_packet_traversal(&peer.p_timers);
    }
    // error:
    m_freem(m);
}

/// `wg_handshake_worker`: the `sc_handshake` task.
pub fn wg_handshake_worker(arg: *mut c_void) {
    let sc = sc_of(arg);
    while let Some(m) = mq_dequeue(&sc.sc_handshake_queue) {
        wg_handshake(sc, m);
    }
}

// The following functions handle encapsulation (encryption) and decapsulation (decryption).
// The wg_{en,de}cap functions will run in the sc_crypt_taskq, while wg_deliver_{in,out} must
// be serialised and will run in nettq.
//
// The packets are tracked in two queues, a serial queue and a parallel queue.
//  - The parallel queue is used to distribute the encryption across multiple threads.
//  - The serial queue ensures that packets are not reordered and are delivered in sequence.
// The wg_tag attached to the packet contains two flags to help the two queues interact.
//  - t_done: The parallel queue has finished with the packet, now the serial queue can do
//            it's work.
//  - t_mbuf: Used to store the *crypted packet. in the case of encryption, this is a newly
//            allocated packet, and in the case of decryption, it is a pointer to the same
//            packet, that has been decrypted and truncated. If t_mbuf is NULL, then *cryption
//            failed and this packet should not be passed.
// wg_{en,de}cap work on the parallel queue, while wg_deliver_{in,out} work on the serial
// queue.

/// `wg_encap`: encrypts a staged packet into a new data message for its peer.
pub fn wg_encap(sc: &WgSoftc, m: &'static Mbuf) {
    let Some(t) = wg_tag_get(m) else {
        m_freem(m);
        return;
    };
    let Some(peer) = t.t_peer.get() else {
        m_freem(m);
        return;
    };

    let pktlen = m.m_pkthdr().len.get() as usize;
    let plaintext_len = wg_pkt_with_padding(pktlen);
    let padding_len = plaintext_len - pktlen;
    let out_len = size_of::<WgPktData>() + plaintext_len + NOISE_AUTHTAG_LEN;

    'error: {
        // For the time being we allocate a new packet with sufficient size to hold the
        // encrypted data and headers. It would be difficult to overcome as p_encap_queue
        // (mbuf_list) holds a reference to the mbuf. If we m_makespace or similar, we risk
        // corrupting that list. Additionally, we only pass a buf and buf length to
        // noise_remote_encrypt. Technically it would be possible to teach noise_remote_encrypt
        // about mbufs, but we would need to sort out the p_encap_queue situation first.
        let Some(mc) = m_clget(
            None,
            M_DONTWAIT,
            (out_len + MAX_HDR.load(Ordering::Relaxed) as usize) as u32,
        ) else {
            break 'error;
        };
        m_align(mc, out_len as i32);

        // SAFETY: `mc`'s one buffer holds `out_len` bytes from its data pointer after
        // `m_align`, and nothing else refers to them yet.
        let data = unsafe { slice::from_raw_parts_mut(mtod::<u8>(mc), out_len) };
        let (hdr, buf) = data.split_at_mut(size_of::<WgPktData>());
        m_copydata(m, 0, &mut buf[..pktlen]);
        buf[pktlen..pktlen + padding_len].fill(0);
        hdr[..4].copy_from_slice(&WG_PKT_DATA.to_ne_bytes());

        // Copy the flow hash from the inner packet to the outer packet, so that fq_codel can
        // properly separate streams, rather than falling back to random buckets.
        mc.m_pkthdr().ph_flowid.set(m.m_pkthdr().ph_flowid.get());

        mc.m_pkthdr().pf.prio.set(m.m_pkthdr().pf.prio.get());

        let mut r_idx = 0u32;
        let mut nonce = 0u64;
        let res = noise_remote_encrypt(&peer.p_remote, &mut r_idx, &mut nonce, buf, plaintext_len);
        hdr[4..8].copy_from_slice(&r_idx.to_ne_bytes());
        // Wire format is little endian.
        hdr[8..16].copy_from_slice(&nonce.to_le_bytes());

        match res {
            Ok(()) => {}
            Err(Errno::EINVAL) => {
                m_freem(mc);
                break 'error;
            }
            Err(Errno::ESTALE) => wg_timers_event_want_initiation(&peer.p_timers),
            Err(e) => panic(format_args!("unexpected result: {}", e as i32)),
        }

        // A packet with length 0 is a keepalive packet
        if pktlen == 0 {
            wgprintf!(
                LOG_DEBUG,
                sc,
                Some(&peer.p_endpoint_mtx),
                "Sending keepalive packet to peer {} ({})\n",
                peer.p_id.get(),
                SaNtop::of(&peer.p_endpoint.get().e_remote)
            );
        }

        mc.m_pkthdr().ph_loopcnt.set(m.m_pkthdr().ph_loopcnt.get());
        mc.m_flags().set(mc.m_flags().get() & !(M_MCAST | M_BCAST));
        mc.m_pkthdr().len.set(out_len as i32);
        mc.m_len().set(out_len as u32);

        // We would count ifc_opackets, ifc_obytes of m here, except if_snd already does that
        // for us, so no need to worry about it.
        // counters_pkt(sc->sc_if.if_counters, ifc_opackets, ifc_obytes, m->m_pkthdr.len);
        wg_peer_counters_add(peer, out_len as u64, 0);

        t.t_mbuf.set(Some(mc));
    }
    // error:
    t.t_done.set(true);
    if let Some(tq) = net_tq(sc.sc_if.if_index.get()) {
        task_add(tq, &peer.p_deliver_out);
    }
}

/// `wg_decap`: decrypts a data message in place, checks its inner source against the peer's
/// allowed IPs and readies it for IPv4.
pub fn wg_decap(sc: &WgSoftc, m: &'static Mbuf) {
    let Some(t) = wg_tag_get(m) else {
        m_freem(m);
        return;
    };
    let Some(peer) = t.t_peer.get() else {
        m_freem(m);
        return;
    };

    'error: {
        // Likewise to wg_encap, we pass a buf and buf length to noise_remote_decrypt. Again,
        // possible to teach it about mbufs but need to get over the p_decap_queue situation
        // first. However, we do not need to allocate a new mbuf as the decrypted packet is
        // strictly smaller than encrypted. We just set t_mbuf to m and wg_deliver_in knows how
        // to deal with that.
        let pktlen = m.m_pkthdr().len.get() as usize;
        // SAFETY: `wg_input` made the packet contiguous (`m_pullup` of its whole length) and
        // checked it is at least a data header and a tag long; the parallel queue owns it.
        let data = unsafe { slice::from_raw_parts_mut(mtod::<u8>(m), pktlen) };
        let (hdr, payload) = data.split_at_mut(size_of::<WgPktData>());
        let r_idx = u32::from_ne_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
        let mut n = [0u8; 8];
        n.copy_from_slice(&hdr[8..16]);
        // Wire format is little endian.
        let nonce = u64::from_le_bytes(n);
        let res = noise_remote_decrypt(&peer.p_remote, r_idx, nonce, payload);

        match res {
            Ok(()) => {}
            Err(Errno::EINVAL) => break 'error,
            Err(Errno::ECONNRESET) => wg_timers_event_handshake_complete(&peer.p_timers),
            Err(Errno::ESTALE) => wg_timers_event_want_initiation(&peer.p_timers),
            Err(e) => panic(format_args!("unexpected response: {}", e as i32)),
        }

        wg_peer_set_endpoint_from_tag(peer, t);

        wg_peer_counters_add(peer, 0, pktlen as u64);

        m_adj(m, size_of::<WgPktData>() as i32);
        m_adj(m, -(NOISE_AUTHTAG_LEN as i32));

        if let Some(c) = sc.sc_if.if_counters.get() {
            counters_pkt(
                c,
                IfCounters::IfcIpackets,
                IfCounters::IfcIbytes,
                m.m_pkthdr().len.get() as u64,
            );
        }

        // A packet with length 0 is a keepalive packet
        if m.m_pkthdr().len.get() == 0 {
            wgprintf!(
                LOG_DEBUG,
                sc,
                Some(&peer.p_endpoint_mtx),
                "Receiving keepalive packet from peer {} ({})\n",
                peer.p_id.get(),
                SaNtop::of(&peer.p_endpoint.get().e_remote)
            );
            // done:
            t.t_mbuf.set(Some(m));
            break 'error;
        }

        // We can let the network stack handle the intricate validation of the IP header, we
        // just worry about the sizeof and the version, so we can read the source address in
        // wg_aip_lookup.
        //
        // We also need to trim the packet, as it was likely padded before encryption. While we
        // could drop it here, it will be more helpful to pass it to bpf_mtap and use the
        // counters that people are expecting in ipv4_input and ipv6_input. We can rely on
        // ipv4_input and ipv6_input to properly validate the headers.
        let len = m.m_pkthdr().len.get() as usize;
        let ip: Option<Ip> = if len >= size_of::<Ip>() {
            // SAFETY: the decrypted packet is contiguous and at least an IP header long.
            Some(unsafe { mtod::<Ip>(m).read_unaligned() })
        } else {
            None
        };
        let allowed_peer = match ip {
            Some(ip) if ip.ip_v() == IPVERSION => {
                m.m_pkthdr().ph_family.set(AF_INET);

                let ip_len = usize::from(ntohs(ip.ip_len));
                if ip_len >= size_of::<Ip>() && ip_len < len {
                    m_adj(m, ip_len as i32 - len as i32);
                }

                wg_aip_lookup(sc.aip4(), &in_bytes(&ip.ip_src))
            }
            _ => match wg_decap_ip6(sc, m, len) {
                Some(allowed_peer) => allowed_peer,
                None => {
                    wgprintf!(
                        LOG_WARNING,
                        sc,
                        Some(&peer.p_endpoint_mtx),
                        "Packet is neither IPv4 nor IPv6 from peer {} ({})\n",
                        peer.p_id.get(),
                        SaNtop::of(&peer.p_endpoint.get().e_remote)
                    );
                    break 'error;
                }
            },
        };

        if !allowed_peer.is_some_and(|p| ptr::eq(p, peer)) {
            wgprintf!(
                LOG_WARNING,
                sc,
                Some(&peer.p_endpoint_mtx),
                "Packet has unallowed source IP from peer {} ({})\n",
                peer.p_id.get(),
                SaNtop::of(&peer.p_endpoint.get().e_remote)
            );
            break 'error;
        }

        // tunneled packet was not offloaded
        m.m_pkthdr().csum_flags.set(0);

        m.m_pkthdr().ph_ifidx.set(sc.sc_if.if_index.get());
        m.m_pkthdr().ph_rtableid.set(sc.sc_if.if_rdomain.get());
        m.m_flags().set(m.m_flags().get() & !(M_MCAST | M_BCAST));
        pf_pkt_addr_changed(m);

        // done:
        t.t_mbuf.set(Some(m));
    }
    // error:
    t.t_done.set(true);
    if let Some(tq) = net_tq(sc.sc_if.if_index.get()) {
        task_add(tq, &peer.p_deliver_in);
    }
}

/// The IPv6 case of `wg_decap`: if `m` (`len` bytes) is an IPv6 packet, trims it to its
/// `ip6_plen`, sets its family and answers the peer the source address belongs to (`Some`,
/// which may be `None` inside); `None` when it is not an IPv6 packet (always, without
/// `INET6`).
#[cfg(feature = "inet6")]
fn wg_decap_ip6(sc: &WgSoftc, m: &'static Mbuf, len: usize) -> Option<Option<&'static WgPeer>> {
    if len < size_of::<Ip6Hdr>() {
        return None;
    }
    // SAFETY: the decrypted packet is contiguous and at least an IPv6 header long.
    let ip6: Ip6Hdr = unsafe { mtod::<Ip6Hdr>(m).read_unaligned() };
    if ip6.ip6_vfc() & IPV6_VERSION_MASK != IPV6_VERSION {
        return None;
    }
    m.m_pkthdr().ph_family.set(AF_INET6);

    let plen = usize::from(ntohs(ip6.ip6_plen)) + size_of::<Ip6Hdr>();
    if plen < len {
        m_adj(m, plen as i32 - len as i32);
    }

    Some(wg_aip_lookup(sc.aip6(), &ip6.ip6_src.s6_addr))
}

/// The IPv6 case of `wg_decap` (without `INET6`: never an IPv6 packet).
#[cfg(not(feature = "inet6"))]
fn wg_decap_ip6(_sc: &WgSoftc, _m: &'static Mbuf, _len: usize) -> Option<Option<&'static WgPeer>> {
    None
}

/// The softc a task was set with.
fn sc_of(arg: *mut c_void) -> &'static WgSoftc {
    // SAFETY: `wg_clone_create` sets the softc's tasks and upcall with the softc as the
    // argument, and `wg_clone_destroy` destroys the peers (with their barriers) before it
    // frees the softc.
    unsafe { &*arg.cast::<WgSoftc>() }
}

/// `wg_encap_worker`: the `sc_encap` task.
pub fn wg_encap_worker(arg: *mut c_void) {
    let sc = sc_of(arg);
    while let Some(m) = wg_ring_dequeue(&sc.sc_encap_ring) {
        wg_encap(sc, m);
    }
}

/// `wg_decap_worker`: the `sc_decap` task.
pub fn wg_decap_worker(arg: *mut c_void) {
    let sc = sc_of(arg);
    while let Some(m) = wg_ring_dequeue(&sc.sc_decap_ring) {
        wg_decap(sc, m);
    }
}

/// `wg_deliver_out`: the `p_deliver_out` task: sends the encrypted packets in order.
pub fn wg_deliver_out(arg: *mut c_void) {
    let peer = peer_of(arg);
    let sc = peer.sc();

    let mut endpoint = wg_peer_get_endpoint(peer);

    while let Some((m, t)) = wg_queue_dequeue(&peer.p_encap_queue) {
        // t_mbuf will contain the encrypted packet
        let Some(mc) = t.t_mbuf.get() else {
            ifp_counters_inc(&sc.sc_if, IfCounters::IfcOerrors);
            m_freem(m);
            continue;
        };

        let ret = wg_send(sc, &endpoint, mc);

        if ret.is_ok() {
            wg_timers_event_any_authenticated_packet_traversal(&peer.p_timers);
            wg_timers_event_any_authenticated_packet_sent(&peer.p_timers);

            if m.m_pkthdr().len.get() != 0 {
                wg_timers_event_data_sent(&peer.p_timers);
            }
        } else if ret == Err(Errno::EADDRNOTAVAIL) {
            wg_peer_clear_src(peer);
            endpoint = wg_peer_get_endpoint(peer);
        }

        m_freem(m);
    }
}

/// `wg_deliver_in`: the `p_deliver_in` task: hands the decrypted packets to IPv4 in order.
pub fn wg_deliver_in(arg: *mut c_void) {
    let peer = peer_of(arg);
    let sc = peer.sc();

    while let Some((m, t)) = wg_queue_dequeue(&peer.p_decap_queue) {
        // t_mbuf will contain the decrypted packet
        let Some(tm) = t.t_mbuf.get() else {
            ifp_counters_inc(&sc.sc_if, IfCounters::IfcIerrors);
            m_freem(m);
            continue;
        };

        // From here on m == t->t_mbuf
        crate::kassert!(ptr::eq(m, tm));

        wg_timers_event_any_authenticated_packet_received(&peer.p_timers);
        wg_timers_event_any_authenticated_packet_traversal(&peer.p_timers);

        if m.m_pkthdr().len.get() == 0 {
            m_freem(m);
            continue;
        }

        let if_bpf = sc.sc_if.if_bpf.get();
        if !if_bpf.is_null() {
            let _ = bpf_mtap_af(
                if_bpf,
                u32::from(m.m_pkthdr().ph_family.get()),
                m,
                BPF_DIRECTION_IN,
            );
        }

        net_lock();
        match m.m_pkthdr().ph_family.get() {
            AF_INET => ipv4_input(&sc.sc_if, m, None),
            #[cfg(feature = "inet6")]
            AF_INET6 => ipv6_input(&sc.sc_if, m, None),
            _ => panic(format_args!("invalid ph_family")),
        }
        net_unlock();

        wg_timers_event_data_received(&peer.p_timers);
    }
}

/// Puts `m` on a ring; `false` when the ring is full.
fn wg_ring_enqueue(r: &WgRing, m: &'static Mbuf) -> bool {
    mtx_enter(&r.r_mtx);
    let ok = r.r_tail.get().wrapping_sub(r.r_head.get()) < MAX_QUEUED_PKT;
    if ok {
        r.r_buf[(r.r_tail.get() & MAX_QUEUED_PKT_MASK) as usize].set(Some(m));
        r.r_tail.set(r.r_tail.get().wrapping_add(1));
    }
    mtx_leave(&r.r_mtx);
    ok
}

/// `wg_queue_in`: queues a received data message on the peer's serial queue and the
/// interface's parallel ring; `ENOBUFS` when either is full.
pub fn wg_queue_in(sc: &WgSoftc, peer: &WgPeer, m: &'static Mbuf) -> Result<(), Errno> {
    let parallel = &sc.sc_decap_ring;
    let serial = &peer.p_decap_queue;

    mtx_enter(&serial.q_mtx);
    if serial.q_list.ml_len.load(Ordering::Relaxed) < MAX_QUEUED_PKT {
        ml_enqueue(&serial.q_list, m);
        mtx_leave(&serial.q_mtx);
    } else {
        mtx_leave(&serial.q_mtx);
        m_freem(m);
        return Err(Errno::ENOBUFS);
    }

    if !wg_ring_enqueue(parallel, m) {
        if let Some(t) = wg_tag_get(m) {
            t.t_done.set(true);
        }
        return Err(Errno::ENOBUFS);
    }

    Ok(())
}

/// `wg_queue_out`: moves the peer's staged packets to its serial queue and the interface's
/// parallel ring.
pub fn wg_queue_out(sc: &WgSoftc, peer: &WgPeer) {
    let parallel = &sc.sc_encap_ring;
    let serial = &peer.p_encap_queue;
    let ml = MbufList::new();
    let ml_free = MbufList::new();

    // We delist all staged packets and then add them to the queues. This can race with
    // wg_qstart when called from wg_send_keepalive, however wg_qstart will not race as it is
    // serialised.
    mq_delist(&peer.p_stage_queue, &ml);
    ml_init(&ml_free);

    while let Some(m) = ml_dequeue(&ml) {
        mtx_enter(&serial.q_mtx);
        if serial.q_list.ml_len.load(Ordering::Relaxed) < MAX_QUEUED_PKT {
            ml_enqueue(&serial.q_list, m);
            mtx_leave(&serial.q_mtx);
        } else {
            mtx_leave(&serial.q_mtx);
            ml_enqueue(&ml_free, m);
            continue;
        }

        if !wg_ring_enqueue(parallel, m)
            && let Some(t) = wg_tag_get(m)
        {
            t.t_done.set(true);
        }
    }

    let dropped = ml_purge(&ml_free);
    if dropped > 0 {
        counters_add(&sc.sc_if, IfCounters::IfcOqdrops, u64::from(dropped));
    }
}

/// `wg_ring_dequeue`.
pub fn wg_ring_dequeue(r: &WgRing) -> Option<&'static Mbuf> {
    let mut m = None;
    mtx_enter(&r.r_mtx);
    if r.r_head.get() != r.r_tail.get() {
        m = r.r_buf[(r.r_head.get() & MAX_QUEUED_PKT_MASK) as usize].take();
        r.r_head.set(r.r_head.get().wrapping_add(1));
    }
    mtx_leave(&r.r_mtx);
    m
}

/// `wg_queue_dequeue`: the head of a serial queue, with its tag, once the parallel queue is
/// done with it.
pub fn wg_queue_dequeue(q: &WgQueue) -> Option<(&'static Mbuf, &'static WgTag)> {
    mtx_enter(&q.q_mtx);
    let mut ret = None;
    if let Some(m) = q.q_list.ml_head.get()
        && let Some(t) = wg_tag_get(m)
        && t.t_done.get()
    {
        let _ = ml_dequeue(&q.q_list);
        ret = Some((m, t));
    }
    mtx_leave(&q.q_mtx);
    ret
}

/// `wg_remote_get`: the `u_remote_get` upcall.
pub fn wg_remote_get(
    arg: *mut c_void,
    public: &[u8; NOISE_PUBLIC_KEY_LEN],
) -> Option<&'static NoiseRemote> {
    let sc = sc_of(arg);
    let peer = wg_peer_lookup(sc, public)?;
    Some(&peer.p_remote)
}

/// `wg_index_set`: the `u_index_set` upcall: a random index, unique in the interface, for the
/// remote's peer.
pub fn wg_index_set(arg: *mut c_void, remote: &'static NoiseRemote) -> u32 {
    let sc = sc_of(arg);

    // We can modify this without a lock as wg_index_set, wg_index_drop are guaranteed to be
    // serialised (per remote).
    let peer = WgPeer::of_remote(remote);
    let Some(index) = peer.p_unused_index.first() else {
        panic(format_args!("wg_index_set: no free index"));
    };
    // SAFETY: the list is not empty (its first element was just read); the remote's
    // handshake lock serialises this with `wg_index_drop`.
    unsafe { peer.p_unused_index.remove_head() };

    index.i_value.set(Some(remote));

    let table = sc.sc_index.get().unwrap_or(&[]);
    mtx_enter(&sc.sc_index_mtx);
    let bucket = loop {
        // assign_id:
        index.i_key.set(arc4random());
        let key = u64::from(index.i_key.get()) & sc.sc_index_mask.get();
        let Some(bucket) = table.get(key as usize) else {
            panic(format_args!("wg_index_set: no index table"));
        };
        if !bucket
            .iter()
            .any(|iter| iter.i_key.get() == index.i_key.get())
        {
            break bucket;
        }
    };

    // SAFETY: the index came off the unused list, so it is on no bucket; `sc_index_mtx` is
    // held.
    unsafe { bucket.insert_head(index) };

    mtx_leave(&sc.sc_index_mtx);

    // Likewise, no need to lock for index here.
    index.i_key.get()
}

/// `wg_index_get`: the remote of a session index.
pub fn wg_index_get(sc: &WgSoftc, key0: u32) -> Option<&'static NoiseRemote> {
    let key = u64::from(key0) & sc.sc_index_mask.get();
    let mut remote = None;

    mtx_enter(&sc.sc_index_mtx);
    if let Some(bucket) = sc.sc_index.get().and_then(|t| t.get(key as usize))
        && let Some(iter) = bucket.iter().find(|iter| iter.i_key.get() == key0)
    {
        remote = iter.i_value.get();
    }
    mtx_leave(&sc.sc_index_mtx);
    remote
}

/// `wg_index_drop`: the `u_index_drop` upcall: frees a session index.
pub fn wg_index_drop(arg: *mut c_void, key0: u32) {
    let sc = sc_of(arg);
    let key = u64::from(key0) & sc.sc_index_mask.get();

    mtx_enter(&sc.sc_index_mtx);
    let found = sc
        .sc_index
        .get()
        .and_then(|t| t.get(key as usize))
        .and_then(|bucket| bucket.iter().find(|iter| iter.i_key.get() == key0));
    if let Some(iter) = found {
        // SAFETY: the index is on this bucket; `sc_index_mtx` is held.
        unsafe { ListHead::<WgIndexes>::remove(iter) };
    }
    mtx_leave(&sc.sc_index_mtx);

    // We expect a peer
    let Some(iter) = found else {
        panic(format_args!("wg_index_drop: index {key0:#x} not found"));
    };
    let Some(remote) = iter.i_value.get() else {
        panic(format_args!(
            "wg_index_drop: index {key0:#x} without remote"
        ));
    };
    let peer = WgPeer::of_remote(remote);
    // SAFETY: the index left the table above and is on no unused list (it was in use).
    unsafe { peer.p_unused_index.insert_head(iter) };
}

/// `wg_input`: the UDP socket's upcall: queues a received datagram (`hlen` bytes of IP and UDP
/// header first) as a handshake message or a data message. Always consumes the packet.
///
/// # Safety
///
/// `arg` is the softc `wg_bind` gave the socket; `ip` (or, with `INET6`, `ip6`) points at the
/// packet's IP header and `uh` at its UDP header, both readable.
pub unsafe fn wg_input(
    arg: *mut c_void,
    m: &'static Mbuf,
    ip: *const Ip,
    ip6: *const c_void,
    uh: *const c_void,
    hlen: i32,
    _ns: Option<&Netstack>,
) -> Option<&'static Mbuf> {
    let sc = sc_of(arg);

    net_assert_locked("wg_input");

    let Some(t) = wg_tag_get(m) else {
        m_freem(m);
        return None;
    };

    if !ip.is_null() {
        // SAFETY: the caller's contract: the IP header and `uh_sport`, the first member of
        // `struct udphdr`, are readable.
        let (ip, sport) = unsafe { (ip.read_unaligned(), uh.cast::<u16>().read_unaligned()) };
        let mut e = t.t_endpoint.get();
        e.e_remote.set_sa_sin(&SockaddrIn {
            sin_len: size_of::<SockaddrIn>() as u8,
            sin_family: AF_INET,
            sin_port: sport,
            sin_addr: ip.ip_src,
            ..SockaddrIn::default()
        });
        e.e_local.set_l_in(ip.ip_dst);
        t.t_endpoint.set(e);
    // SAFETY: the caller's contract: `ip6` is null or a readable IPv6 header, `uh` a UDP header.
    } else if !unsafe { wg_input_ip6(t, ip6, uh) } {
        m_freem(m);
        return None;
    }

    // m has a IP/IPv6 header of hlen length, we don't need it anymore.
    m_adj(m, hlen);

    // Ensure mbuf is contiguous over full length of packet. This is done so we can directly
    // read the handshake values in wg_handshake, and so we can decrypt a transport packet by
    // passing a single buffer to noise_remote_decrypt in wg_decap.
    let m = m_pullup(m, m.m_pkthdr().len.get())?;

    let len = m.m_pkthdr().len.get() as usize;
    let ty = if len >= size_of::<u32>() {
        // SAFETY: the packet is contiguous and at least a word long.
        unsafe { mtod::<u32>(m).read_unaligned() }
    } else {
        0
    };

    if (len == size_of::<WgPktInitiation>() && ty == WG_PKT_INITIATION)
        || (len == size_of::<WgPktResponse>() && ty == WG_PKT_RESPONSE)
        || (len == size_of::<WgPktCookie>() && ty == WG_PKT_COOKIE)
    {
        if mq_enqueue(&sc.sc_handshake_queue, m) {
            wgprintf!(
                LOG_DEBUG,
                sc,
                None,
                "Dropping handshakepacket from {}\n",
                SaNtop::of(&t.t_endpoint.get().e_remote)
            );
        }
        task_add(wg_handshake_taskq(), &sc.sc_handshake);
    } else if len >= size_of::<WgPktData>() + NOISE_AUTHTAG_LEN && ty == WG_PKT_DATA {
        // SAFETY: the packet is contiguous and at least a data header long.
        let data: WgPktData = unsafe { pkt_read(m) };

        if let Some(remote) = wg_index_get(sc, data.r_idx) {
            let peer = WgPeer::of_remote(remote);
            t.t_peer.set(Some(peer));
            t.t_mbuf.set(None);
            t.t_done.set(false);

            if wg_queue_in(sc, peer, m).is_err() {
                ifp_counters_inc(&sc.sc_if, IfCounters::IfcIqdrops);
            }
            task_add(wg_crypt_taskq(), &sc.sc_decap);
        } else {
            ifp_counters_inc(&sc.sc_if, IfCounters::IfcIerrors);
            m_freem(m);
        }
    } else {
        ifp_counters_inc(&sc.sc_if, IfCounters::IfcIerrors);
        m_freem(m);
    }

    None
}

/// The IPv6 case of `wg_input`: fills the tag's endpoint from the datagram's IPv6 header and
/// UDP source port; `false` when there is no IPv6 header (always, without `INET6`).
///
/// # Safety
///
/// `ip6` is null or points at a readable IPv6 header, `uh` at a readable UDP header.
#[cfg(feature = "inet6")]
unsafe fn wg_input_ip6(t: &WgTag, ip6: *const c_void, uh: *const c_void) -> bool {
    if ip6.is_null() {
        return false;
    }
    // SAFETY: the caller's contract: the IPv6 header and `uh_sport`, the first member of
    // `struct udphdr`, are readable.
    let (ip6, sport) = unsafe {
        (
            ip6.cast::<Ip6Hdr>().read_unaligned(),
            uh.cast::<u16>().read_unaligned(),
        )
    };
    let mut e = t.t_endpoint.get();
    e.e_remote.set_sa_sin6(&SockaddrIn6 {
        sin6_len: size_of::<SockaddrIn6>() as u8,
        sin6_family: AF_INET6,
        sin6_port: sport,
        sin6_addr: ip6.ip6_src,
        ..SockaddrIn6::default()
    });
    e.e_local.set_l_in6(ip6.ip6_dst);
    t.t_endpoint.set(e);
    true
}

/// The IPv6 case of `wg_input` (without `INET6`: no IPv6 header).
///
/// # Safety
///
/// Nothing is read.
#[cfg(not(feature = "inet6"))]
unsafe fn wg_input_ip6(_t: &WgTag, _ip6: *const c_void, _uh: *const c_void) -> bool {
    false
}

/// `wg_qstart`: the interface's `if_qstart`: stages the packets of the send queue on their
/// peers, then queues them for encryption (or asks for a handshake).
pub fn wg_qstart(ifq: &'static Ifqueue) {
    let Some(ifp) = ifq.ifq_if.get() else {
        return;
    };
    let sc = WgSoftc::of_ifp(ifp);
    let start_list: SlistHead<WgPeerStart> = SlistHead::new();

    // We should be OK to modify p_start_list, p_start_onlist in this function as there should
    // only be one ifp->if_qstart invoked at a time.
    while let Some(m) = ifq_dequeue(ifq) {
        let Some(peer) = wg_tag_get(m).and_then(|t| t.t_peer.get()) else {
            m_freem(m);
            continue;
        };

        let if_bpf = sc.sc_if.if_bpf.get();
        if !if_bpf.is_null() {
            let _ = bpf_mtap_af(
                if_bpf,
                u32::from(m.m_pkthdr().ph_family.get()),
                m,
                BPF_DIRECTION_OUT,
            );
        }

        if mq_push(&peer.p_stage_queue, m) {
            ifp_counters_inc(ifp, IfCounters::IfcOqdrops);
        }
        if !peer.p_start_onlist.get() {
            // SAFETY: a peer is on the start list only while `p_start_onlist` is set, and this
            // function, which clears it below, is serialised.
            unsafe { start_list.insert_head(peer) };
            peer.p_start_onlist.set(true);
        }
    }
    for peer in start_list.iter() {
        if noise_remote_ready(&peer.p_remote).is_ok() {
            wg_queue_out(sc, peer);
        } else {
            wg_timers_event_want_initiation(&peer.p_timers);
        }
        peer.p_start_onlist.set(false);
    }
    task_add(wg_crypt_taskq(), &sc.sc_encap);
}

/// The IPv6 case of `wg_output`: the peer `ip6_dst` of the packet belongs to (`Some`, which
/// may be `None` inside); `None` when `family` is not `AF_INET6` (always, without `INET6`).
#[cfg(feature = "inet6")]
fn wg_output_ip6(
    sc: &WgSoftc,
    m: &'static Mbuf,
    family: SaFamily,
) -> Option<Option<&'static WgPeer>> {
    if family != AF_INET6 {
        return None;
    }
    // SAFETY: an IPv6 packet starts with its header in the first mbuf (`ip6_output`).
    let ip6: Ip6Hdr = unsafe { mtod::<Ip6Hdr>(m).read_unaligned() };
    Some(wg_aip_lookup(sc.aip6(), &ip6.ip6_dst.s6_addr))
}

/// The IPv6 case of `wg_output` (without `INET6`: never).
#[cfg(not(feature = "inet6"))]
fn wg_output_ip6(
    _sc: &WgSoftc,
    _m: &'static Mbuf,
    _family: SaFamily,
) -> Option<Option<&'static WgPeer>> {
    None
}

/// `wg_output`: the interface's `if_output`: finds the peer of the destination and enqueues
/// the packet for it.
///
/// # Safety
///
/// As for `if_output` (`IfOutputFn`).
pub unsafe fn wg_output(
    ifp: &'static Ifnet,
    m: &'static Mbuf,
    sa: *const Sockaddr,
    _rt: Option<&'static Rtentry>,
) -> Result<(), Errno> {
    let sc = WgSoftc::of_ifp(ifp);

    net_assert_locked("wg_output");

    let ret = 'error: {
        let Some(t) = wg_tag_get(m) else {
            break 'error Err(Errno::ENOBUFS);
        };

        // SAFETY: the caller's contract: `sa` is a readable socket address.
        let family = unsafe { (*sa).sa_family };
        m.m_pkthdr().ph_family.set(family);
        let peer = if family == AF_INET {
            // SAFETY: an IPv4 packet starts with its header in the first mbuf (`ip_output`).
            let ip: Ip = unsafe { mtod::<Ip>(m).read_unaligned() };
            wg_aip_lookup(sc.aip4(), &in_bytes(&ip.ip_dst))
        } else if let Some(peer) = wg_output_ip6(sc, m, family) {
            peer
        } else {
            break 'error Err(Errno::EAFNOSUPPORT);
        };

        let Some(peer) = peer else {
            break 'error Err(Errno::ENETUNREACH);
        };

        let af = peer.p_endpoint.get().e_remote.sa_family();
        if af != AF_INET && af != AF_INET6 {
            wgprintf!(
                LOG_DEBUG,
                sc,
                None,
                "No valid endpoint has been configured or discovered for peer {}\n",
                peer.p_id.get()
            );
            break 'error Err(Errno::EDESTADDRREQ);
        }

        let loopcnt = m.m_pkthdr().ph_loopcnt.get();
        m.m_pkthdr().ph_loopcnt.set(loopcnt.wrapping_add(1));
        if loopcnt > M_MAXLOOP {
            wgprintf!(LOG_DEBUG, sc, None, "Packet looped\n");
            break 'error Err(Errno::ELOOP);
        }

        // As we hold a reference to peer in the mbuf, we can't handle a delayed packet without
        // doing some refcnting. If a peer is removed while a delayed holds a reference, bad
        // things will happen. For the time being, delayed packets are unsupported. This may be
        // fixed with another aip_lookup in wg_qstart, or refcnting as mentioned before.
        if m.m_pkthdr().pf.delay.get() > 0 {
            wgprintf!(LOG_DEBUG, sc, None, "PF delay unsupported\n");
            break 'error Err(Errno::EOPNOTSUPP);
        }

        t.t_peer.set(Some(peer));
        t.t_mbuf.set(None);
        t.t_done.set(false);
        t.t_mtu.set(ifp.if_mtu.get() as i32);

        // We still have an issue with ifq that will count a packet that gets dropped in
        // wg_qstart, or not encrypted. These get counted as ofails or oqdrops, so the packet
        // gets counted twice.
        return if_enqueue(ifp, m);
    };
    // error:
    ifp_counters_inc(ifp, IfCounters::IfcOerrors);
    m_freem(m);
    ret
}

/// `wg_ioctl_set`: `SIOCSWG`: the identity, port, routing table and peers the user describes
/// at `data.wgd_interface`.
pub fn wg_ioctl_set(sc: &'static WgSoftc, data: &mut WgDataIo) -> Result<(), Errno> {
    let mut iface_o = WgInterfaceIo::default();
    let mut peer_o = WgPeerIo::default();
    let mut aip_o = WgAipIo::default();
    let mut public = [0u8; WG_KEY_SIZE];
    let mut private = [0u8; WG_KEY_SIZE];

    let Some(p) = curproc() else {
        return Err(Errno::EPERM);
    };
    suser(p)?;

    rw_enter_write(&sc.sc_lock);

    let iface_p = data.wgd_interface;
    let ret = 'error: {
        iface_o = match copyin_obj::<WgInterfaceIo>(iface_p) {
            Ok(v) => v,
            Err(e) => break 'error Err(e),
        };

        if iface_o.i_flags & WG_INTERFACE_REPLACE_PEERS != 0 {
            for peer in sc.sc_peer_seq.iter() {
                wg_peer_destroy(peer);
            }
        }

        if iface_o.i_flags & WG_INTERFACE_HAS_PRIVATE != 0
            && (noise_local_keys(&sc.sc_local, None, Some(&mut private)).is_err()
                || timingsafe_bcmp(&private, &iface_o.i_private))
        {
            if curve25519_generate_public(&mut public, &iface_o.i_private)
                && let Some(peer) = wg_peer_lookup(sc, &public)
            {
                wg_peer_destroy(peer);
            }
            noise_local_lock_identity(&sc.sc_local);
            let has_identity = noise_local_set_private(&sc.sc_local, &iface_o.i_private);
            for peer in sc.sc_peer_seq.iter() {
                noise_remote_precompute(&peer.p_remote);
                wg_timers_event_reset_handshake_last_sent(&peer.p_timers);
                noise_remote_expire_current(&peer.p_remote);
            }
            cookie_checker_update(
                &sc.sc_cookie,
                if has_identity.is_ok() {
                    Some(&public)
                } else {
                    None
                },
            );
            noise_local_unlock_identity(&sc.sc_local);
        }

        let mut port = if iface_o.i_flags & WG_INTERFACE_HAS_PORT != 0 {
            htons(iface_o.i_port)
        } else {
            sc.sc_udp_port.get()
        };

        let mut rtable = if iface_o.i_flags & WG_INTERFACE_HAS_RTABLE != 0 {
            iface_o.i_rtable
        } else {
            sc.sc_udp_rtable.get()
        };

        if port != sc.sc_udp_port.get() || rtable != sc.sc_udp_rtable.get() {
            for peer in sc.sc_peer_seq.iter() {
                wg_peer_clear_src(peer);
            }

            if sc.sc_if.if_flags.get() & IFF_RUNNING != 0
                && let Err(e) = wg_bind(sc, &mut port, &mut rtable)
            {
                break 'error Err(e);
            }

            sc.sc_udp_port.set(port);
            sc.sc_udp_rtable.set(rtable);
        }

        let mut peer_p = iface_p.wrapping_add(size_of::<WgInterfaceIo>());
        'peers: for _ in 0..iface_o.i_peers_count {
            peer_o = match copyin_obj::<WgPeerIo>(peer_p) {
                Ok(v) => v,
                Err(e) => break 'error Err(e),
            };

            'next_peer: {
                // Peer must have public key
                if peer_o.p_flags & WG_PEER_HAS_PUBLIC == 0 {
                    break 'next_peer;
                }

                // 0 = latest protocol, 1 = this protocol
                if peer_o.p_protocol_version != 0 && peer_o.p_protocol_version > 1 {
                    break 'error Err(Errno::EPFNOSUPPORT);
                }

                // Get local public and check that peer key doesn't match
                if noise_local_keys(&sc.sc_local, Some(&mut public), None).is_ok()
                    && public == peer_o.p_public
                {
                    break 'next_peer;
                }

                // Lookup peer, or create if it doesn't exist
                let peer = match wg_peer_lookup(sc, &peer_o.p_public) {
                    Some(peer) => peer,
                    None => {
                        // If we want to delete, no need creating a new one. Also, don't create
                        // a new one if we only want to update.
                        if peer_o.p_flags & (WG_PEER_REMOVE | WG_PEER_UPDATE) != 0 {
                            break 'next_peer;
                        }

                        match wg_peer_create(sc, &peer_o.p_public) {
                            Some(peer) => peer,
                            None => break 'error Err(Errno::ENOMEM),
                        }
                    }
                };

                // Remove peer and continue if specified
                if peer_o.p_flags & WG_PEER_REMOVE != 0 {
                    wg_peer_destroy(peer);
                    break 'next_peer;
                }

                if peer_o.p_flags & WG_PEER_HAS_ENDPOINT != 0 {
                    wg_peer_set_sockaddr(peer, &peer_o.p_endpoint);
                }

                if peer_o.p_flags & WG_PEER_HAS_PSK != 0 {
                    let _ = noise_remote_set_psk(&peer.p_remote, &peer_o.p_psk);
                }

                if peer_o.p_flags & WG_PEER_HAS_PKA != 0 {
                    wg_timers_set_persistent_keepalive(&peer.p_timers, peer_o.p_pka);
                }

                if peer_o.p_flags & WG_PEER_REPLACE_AIPS != 0 {
                    for aip in peer.p_aip.iter() {
                        let _ = wg_aip_remove(sc, peer, &aip.a_data.get());
                    }
                }

                if peer_o.p_flags & WG_PEER_SET_DESCRIPTION != 0 {
                    let mut descr = [0u8; IFDESCRSIZE];
                    strlcpy(&mut descr, &peer_o.p_description);
                    peer.p_description.set(descr);
                }

                let mut aip_p = peer_p.wrapping_add(size_of::<WgPeerIo>());
                for _ in 0..peer_o.p_aips_count {
                    aip_o = match copyin_obj::<WgAipIo>(aip_p) {
                        Ok(v) => v,
                        Err(e) => break 'error Err(e),
                    };
                    if let Err(e) = wg_aip_add(sc, peer, &aip_o) {
                        break 'error Err(e);
                    }
                    aip_p = aip_p.wrapping_add(size_of::<WgAipIo>());
                }

                peer_p = aip_p;
                continue 'peers;
            }
            // next_peer:
            let aip_p = peer_p
                .wrapping_add(size_of::<WgPeerIo>())
                .wrapping_add(peer_o.p_aips_count.wrapping_mul(size_of::<WgAipIo>()));
            peer_p = aip_p;
        }
        Ok(())
    };

    rw_exit_write(&sc.sc_lock);
    crate::crypto::wipe(&mut iface_o);
    crate::crypto::wipe(&mut peer_o);
    crate::crypto::wipe(&mut aip_o);
    explicit_bzero(&mut public);
    explicit_bzero(&mut private);
    ret
}

/// `wg_ioctl_get`: `SIOCGWG`: the configuration (the keys and peers for the superuser only),
/// copied out when `data.wgd_size` is large enough; `wgd_size` gets the size it needs.
pub fn wg_ioctl_get(sc: &WgSoftc, data: &mut WgDataIo) -> Result<(), Errno> {
    let mut iface_o = WgInterfaceIo::default();
    let mut peer_o = WgPeerIo::default();
    let mut ret = Ok(());
    let is_suser = curproc().is_some_and(|p| suser(p).is_ok());

    let mut size = size_of::<WgInterfaceIo>();
    'ret_size: {
        if data.wgd_size < size && !is_suser {
            break 'ret_size;
        }

        let iface_p = data.wgd_interface;

        rw_enter_read(&sc.sc_lock);

        'unlock_and_ret_size: {
            if sc.sc_udp_port.get() != 0 {
                iface_o.i_port = ntohs(sc.sc_udp_port.get());
                iface_o.i_flags |= WG_INTERFACE_HAS_PORT;
            }

            if sc.sc_udp_rtable.get() != 0 {
                iface_o.i_rtable = sc.sc_udp_rtable.get();
                iface_o.i_flags |= WG_INTERFACE_HAS_RTABLE;
            }

            'copy_out_iface: {
                if !is_suser {
                    break 'copy_out_iface;
                }

                if noise_local_keys(
                    &sc.sc_local,
                    Some(&mut iface_o.i_public),
                    Some(&mut iface_o.i_private),
                )
                .is_ok()
                {
                    iface_o.i_flags |= WG_INTERFACE_HAS_PUBLIC;
                    iface_o.i_flags |= WG_INTERFACE_HAS_PRIVATE;
                }

                size += size_of::<WgPeerIo>() * sc.sc_peer_num.get();
                size += size_of::<WgAipIo>() * sc.sc_aip_num.get();
                if data.wgd_size < size {
                    break 'unlock_and_ret_size;
                }

                let mut peer_count = 0usize;
                let mut peer_p = iface_p.wrapping_add(size_of::<WgInterfaceIo>());
                for peer in sc.sc_peer_seq.iter() {
                    peer_o = WgPeerIo {
                        p_flags: WG_PEER_HAS_PUBLIC,
                        p_protocol_version: 1,
                        ..WgPeerIo::default()
                    };

                    if noise_remote_keys(
                        &peer.p_remote,
                        Some(&mut peer_o.p_public),
                        Some(&mut peer_o.p_psk),
                    )
                    .is_ok()
                    {
                        peer_o.p_flags |= WG_PEER_HAS_PSK;
                    }

                    if wg_timers_get_persistent_keepalive(&peer.p_timers, &mut peer_o.p_pka).is_ok()
                    {
                        peer_o.p_flags |= WG_PEER_HAS_PKA;
                    }

                    if wg_peer_get_sockaddr(peer, &mut peer_o.p_endpoint).is_ok() {
                        peer_o.p_flags |= WG_PEER_HAS_ENDPOINT;
                    }

                    mtx_enter(&peer.p_counters_mtx);
                    peer_o.p_txbytes = peer.p_counters_tx.get();
                    peer_o.p_rxbytes = peer.p_counters_rx.get();
                    mtx_leave(&peer.p_counters_mtx);

                    peer_o.p_last_handshake = wg_timers_get_last_handshake(&peer.p_timers);

                    let mut aip_count = 0usize;
                    let mut aip_p = peer_p.wrapping_add(size_of::<WgPeerIo>());
                    for aip in peer.p_aip.iter() {
                        if let Err(e) = copyout_obj(&aip.a_data.get(), aip_p) {
                            ret = Err(e);
                            break 'unlock_and_ret_size;
                        }
                        aip_p = aip_p.wrapping_add(size_of::<WgAipIo>());
                        aip_count += 1;
                    }
                    peer_o.p_aips_count = aip_count;

                    strlcpy(&mut peer_o.p_description, &peer.p_description.get());

                    if let Err(e) = copyout_obj(&peer_o, peer_p) {
                        ret = Err(e);
                        break 'unlock_and_ret_size;
                    }

                    peer_p = aip_p;
                    peer_count += 1;
                }
                iface_o.i_peers_count = peer_count;
            }
            // copy_out_iface:
            ret = copyout_obj(&iface_o, iface_p);
        }
        // unlock_and_ret_size:
        rw_exit_read(&sc.sc_lock);
        crate::crypto::wipe(&mut iface_o);
        crate::crypto::wipe(&mut peer_o);
    }
    // ret_size:
    data.wgd_size = size;
    ret
}

/// `wg_ioctl`: the interface's `if_ioctl`.
///
/// # Safety
///
/// As for `if_ioctl` (`IfIoctlFn`).
pub unsafe fn wg_ioctl(ifp: &'static Ifnet, cmd: u64, data: *mut u8) -> Result<(), Errno> {
    let sc = WgSoftc::of_ifp(ifp);

    match cmd {
        SIOCSWG => {
            net_unlock();
            // SAFETY: the caller's contract: `SIOCSWG` takes a `struct wg_data_io`, aligned.
            let ret = wg_ioctl_set(sc, unsafe { &mut *data.cast::<WgDataIo>() });
            net_lock();
            ret
        }
        SIOCGWG => {
            net_unlock();
            // SAFETY: as above, for `SIOCGWG`.
            let ret = wg_ioctl_get(sc, unsafe { &mut *data.cast::<WgDataIo>() });
            net_lock();
            ret
        }
        // Interface IOCTLs
        SIOCSIFADDR | SIOCSIFFLAGS => {
            if cmd == SIOCSIFADDR {
                ifp.if_flags.set(ifp.if_flags.get() | IFF_UP);
            }
            // FALLTHROUGH
            if ifp.if_flags.get() & IFF_UP != 0 {
                wg_up(sc)
            } else {
                wg_down(sc);
                Ok(())
            }
        }
        SIOCSIFMTU => {
            // SAFETY: the caller's contract: `SIOCSIFMTU` takes a `struct ifreq`.
            let ifr = unsafe { &*data.cast::<Ifreq>() };
            // Arbitrary limits
            if ifr.ifr_mtu() <= 0 || ifr.ifr_mtu() > 9000 {
                Err(Errno::EINVAL)
            } else {
                ifp.if_mtu.set(ifr.ifr_mtu() as u32);
                Ok(())
            }
        }
        SIOCADDMULTI | SIOCDELMULTI => Ok(()),
        _ => Err(Errno::ENOTTY),
    }
}

/// `wg_up`: binds the sockets and starts the peers. Called with the net lock held.
pub fn wg_up(sc: &WgSoftc) -> Result<(), Errno> {
    let mut ret = Ok(());

    net_assert_locked("wg_up");
    // We use IFF_RUNNING as an exclusive access here. We also may want an exclusive sc_lock as
    // wg_bind may write to sc_udp_port. We also want to drop NET_LOCK as we want to call
    // socreate, sobind, etc. Once solock is no longer === NET_LOCK, we may be able to avoid
    // this.
    if sc.sc_if.if_flags.get() & IFF_RUNNING == 0 {
        sc.sc_if.if_flags.set(sc.sc_if.if_flags.get() | IFF_RUNNING);
        net_unlock();

        rw_enter_write(&sc.sc_lock);
        // If we successfully bind the socket, then enable the timers for the peer. This will
        // send all staged packets and a keepalive if necessary.
        let mut port = sc.sc_udp_port.get();
        let mut rtable = sc.sc_udp_rtable.get();
        ret = wg_bind(sc, &mut port, &mut rtable);
        if ret.is_ok() {
            sc.sc_udp_port.set(port);
            sc.sc_udp_rtable.set(rtable);
            for peer in sc.sc_peer_seq.iter() {
                wg_timers_enable(&peer.p_timers);
                wg_queue_out(sc, peer);
            }
        }
        rw_exit_write(&sc.sc_lock);

        net_lock();
        if ret.is_err() {
            sc.sc_if
                .if_flags
                .set(sc.sc_if.if_flags.get() & !IFF_RUNNING);
        }
    }
    ret
}

/// `wg_down`: stops the peers, forgets their sessions and closes the sockets. Called with the
/// net lock held.
pub fn wg_down(sc: &WgSoftc) {
    net_assert_locked("wg_down");
    if sc.sc_if.if_flags.get() & IFF_RUNNING == 0 {
        return;
    }
    sc.sc_if
        .if_flags
        .set(sc.sc_if.if_flags.get() & !IFF_RUNNING);
    net_unlock();

    // We only need a read lock here, as we aren't writing to anything that isn't granularly
    // locked.
    rw_enter_read(&sc.sc_lock);
    for peer in sc.sc_peer_seq.iter() {
        mq_purge(&peer.p_stage_queue);
        wg_timers_disable(&peer.p_timers);
    }

    taskq_barrier(wg_handshake_taskq());
    for peer in sc.sc_peer_seq.iter() {
        noise_remote_clear(&peer.p_remote);
        wg_timers_event_reset_handshake_last_sent(&peer.p_timers);
    }

    wg_unbind(sc);
    rw_exit_read(&sc.sc_lock);
    net_lock();
}

/// `wg_clone_create`: creates `wg<unit>`.
pub fn wg_clone_create(_ifc: &'static IfClone, unit: i32) -> Result<(), Errno> {
    kernel_assert_locked();

    if WG_COUNTER.load(Ordering::Relaxed) == 0 {
        let handshake = taskq_create(b"wg_handshake", 2, IPL_NET, TASKQ_MPSAFE);
        let crypt = taskq_create(
            b"wg_crypt",
            NCPUS.load(Ordering::Relaxed) as u32,
            IPL_NET,
            TASKQ_MPSAFE,
        );

        match (handshake, crypt) {
            (Some(h), Some(c)) => {
                WG_HANDSHAKE_TASKQ.store(ptr::from_ref(h).cast_mut(), Ordering::Release);
                WG_CRYPT_TASKQ.store(ptr::from_ref(c).cast_mut(), Ordering::Release);
            }
            (h, c) => {
                for tq in [h, c].into_iter().flatten() {
                    // SAFETY: a queue just created here, used by nobody.
                    unsafe { taskq_destroy(NonNull::from(tq)) };
                }
                WG_HANDSHAKE_TASKQ.store(ptr::null_mut(), Ordering::Release);
                WG_CRYPT_TASKQ.store(ptr::null_mut(), Ordering::Release);
                return Err(Errno::ENOTRECOVERABLE);
            }
        }
    }
    WG_COUNTER.fetch_add(1, Ordering::Relaxed);

    let Some(p) = malloc(size_of::<WgSoftc>(), M_DEVBUF, M_NOWAIT | M_ZERO) else {
        // ret_00:
        return Err(Errno::ENOBUFS);
    };
    let p = p.cast::<WgSoftc>();
    // SAFETY: a zero-filled block of `size_of::<WgSoftc>()` bytes, aligned for it (malloc's
    // chunks are aligned to their size); `sc_local` is written whole before any reference to
    // the softc exists, and every other member is valid all zero (see the module's
    // deviations). It lives until `wg_clone_destroy` frees it.
    let sc: &'static WgSoftc = unsafe {
        ptr::addr_of_mut!((*p.as_ptr()).sc_local).write(NoiseLocal::new());
        p.as_ref()
    };

    let local_upcall = NoiseUpcall {
        u_arg: p.as_ptr().cast(),
        u_remote_get: wg_remote_get,
        u_index_set: wg_index_set,
        u_index_drop: wg_index_drop,
    };

    sc.sc_peer_seq.init();

    // sc_if is initialised after everything else
    let (mut k0, mut k1) = ([0u8; 8], [0u8; 8]);
    arc4random_buf(&mut k0);
    arc4random_buf(&mut k1);
    sc.sc_secret.set(SiphashKey {
        k0: u64::from_ne_bytes(k0),
        k1: u64::from_ne_bytes(k1),
    });

    rw_init(&sc.sc_lock, "wg");
    noise_local_init(&sc.sc_local, &local_upcall);
    'ret_01: {
        if cookie_checker_init(&sc.sc_cookie, &WG_RATELIMIT_POOL).is_err() {
            break 'ret_01;
        }
        sc.sc_udp_port.set(0);
        sc.sc_udp_rtable.set(0);

        rw_init(&sc.sc_so_lock, "wg_so");
        sc.sc_so4.set(None);
        #[cfg(feature = "inet6")]
        sc.sc_so6.set(None);

        sc.sc_aip_num.set(0);
        rw_init(&sc.sc_aip_lock, "wgaip");
        'ret_02: {
            let Some(aip4) = art_alloc(32) else {
                break 'ret_02;
            };
            sc.sc_aip4.set(Some(aip4));
            #[cfg(feature = "inet6")]
            match art_alloc(128) {
                Some(aip6) => sc.sc_aip6.set(Some(aip6)),
                None => {
                    // ret_03:
                    sc.sc_aip4.set(None);
                    free(NonNull::from(aip4).cast(), M_RTABLE, size_of::<Art>());
                    break 'ret_02;
                }
            }

            rw_init(&sc.sc_peer_lock, "wg_peer");
            sc.sc_peer_num.set(0);
            'ret_04: {
                let Some(peers) = hashinit::<WgPeersByKey>(HASHTABLE_PEER_SIZE, M_DEVBUF, M_NOWAIT)
                else {
                    break 'ret_04;
                };
                sc.sc_peer.set(Some(peers));
                sc.sc_peer_mask.set(peers.len() as u64 - 1);

                mtx_init(&sc.sc_index_mtx, IPL_NET);
                let Some(index) = hashinit::<WgIndexes>(HASHTABLE_INDEX_SIZE, M_DEVBUF, M_NOWAIT)
                else {
                    // ret_05:
                    sc.sc_peer.set(None);
                    // SAFETY: the table made just above, empty and reachable no more.
                    unsafe { hashfree(peers, HASHTABLE_PEER_SIZE, M_DEVBUF) };
                    break 'ret_04;
                };
                sc.sc_index.set(Some(index));
                sc.sc_index_mask.set(index.len() as u64 - 1);

                let arg = p.as_ptr().cast::<c_void>();
                task_set(&sc.sc_handshake, wg_handshake_worker, arg);
                mq_init(&sc.sc_handshake_queue, MAX_QUEUED_HANDSHAKES, IPL_NET);

                task_set(&sc.sc_encap, wg_encap_worker, arg);
                task_set(&sc.sc_decap, wg_decap_worker, arg);

                for ring in [&sc.sc_encap_ring, &sc.sc_decap_ring] {
                    // bzero(&sc->sc_*_ring, sizeof(...)): M_ZERO did.
                    ring.r_head.set(0);
                    ring.r_tail.set(0);
                    mtx_init(&ring.r_mtx, IPL_NET);
                }

                // We've setup the softc, now we can setup the ifnet
                let ifp = &sc.sc_if;
                ifp.if_softc.set(arg);

                let mut xname = [0u8; IFNAMSIZ];
                let _ = snprintf(&mut xname, format_args!("wg{unit}"));
                ifp.if_xname.set(xname);

                ifp.if_mtu.set(DEFAULT_MTU);
                ifp.if_flags.set(IFF_BROADCAST | IFF_MULTICAST | IFF_NOARP);
                ifp.if_xflags.set(IFXF_CLONED | IFXF_MPSAFE);
                ifp.if_txmit.set(64); // Keep our workers active for longer.

                ifp.if_ioctl.set(Some(wg_ioctl));
                ifp.if_qstart.set(Some(wg_qstart));
                ifp.if_output.set(Some(wg_output));

                ifp.if_type.set(IFT_WIREGUARD);
                ifp.if_rtrequest.set(Some(p2p_rtrequest));

                if_counters_alloc(ifp);
                if_attach(ifp);
                if_alloc_sadl(ifp);

                bpfattach(&ifp.if_bpf, ifp, DLT_LOOP, size_of::<u32>() as u32);

                wgprintf!(LOG_INFO, sc, None, "Interface created\n");

                return Ok(());
            }
            // ret_04:
            #[cfg(feature = "inet6")]
            if let Some(aip6) = sc.sc_aip6.take() {
                free(NonNull::from(aip6).cast(), M_RTABLE, size_of::<Art>());
            }
            sc.sc_aip4.set(None);
            free(NonNull::from(aip4).cast(), M_RTABLE, size_of::<Art>());
        }
        // ret_02:
        cookie_checker_deinit(&sc.sc_cookie);
    }
    // ret_01:
    free(p.cast(), M_DEVBUF, size_of::<WgSoftc>());
    // ret_00:
    Err(Errno::ENOBUFS)
}

/// `wg_clone_destroy`: destroys a `wg` interface and its peers.
pub fn wg_clone_destroy(ifp: &'static Ifnet) -> Result<(), Errno> {
    let sc = WgSoftc::of_ifp(ifp);

    kernel_assert_locked();

    rw_enter_write(&sc.sc_lock);
    for peer in sc.sc_peer_seq.iter() {
        wg_peer_destroy(peer);
    }
    rw_exit_write(&sc.sc_lock);

    wg_unbind(sc);
    if_detach(ifp);

    if WG_COUNTER.fetch_sub(1, Ordering::Relaxed) == 1 {
        let h = WG_HANDSHAKE_TASKQ.swap(ptr::null_mut(), Ordering::AcqRel);
        let c = WG_CRYPT_TASKQ.swap(ptr::null_mut(), Ordering::AcqRel);
        crate::kassert!(!h.is_null() && !c.is_null());
        for tq in [h, c] {
            if let Some(tq) = NonNull::new(tq) {
                // SAFETY: the queues of the last interface, created by `wg_clone_create`; no
                // peer is left to use them.
                unsafe { taskq_destroy(tq) };
            }
        }
    }

    wgprintf!(LOG_INFO, sc, None, "Interface destroyed\n");

    if let Some(index) = sc.sc_index.take() {
        // SAFETY: the table `wg_clone_create` made; every index was dropped with its peer.
        unsafe { hashfree(index, HASHTABLE_INDEX_SIZE, M_DEVBUF) };
    }
    if let Some(peers) = sc.sc_peer.take() {
        // SAFETY: the table `wg_clone_create` made; every peer was destroyed above.
        unsafe { hashfree(peers, HASHTABLE_PEER_SIZE, M_DEVBUF) };
    }
    #[cfg(feature = "inet6")]
    if let Some(aip6) = sc.sc_aip6.take() {
        free(NonNull::from(aip6).cast(), M_RTABLE, size_of::<Art>());
    }
    if let Some(aip4) = sc.sc_aip4.take() {
        free(NonNull::from(aip4).cast(), M_RTABLE, size_of::<Art>());
    }
    cookie_checker_deinit(&sc.sc_cookie);
    free(NonNull::from(sc).cast(), M_DEVBUF, size_of::<WgSoftc>());
    Ok(())
}

/// `wgattach`: the pseudo-device attach function: registers the cloner and makes the pools.
pub fn wgattach(_nwg: i32) {
    // WGTEST: cookie_test() and noise_test() are the host test modules.

    // SAFETY: `wgattach` runs once, from `main`'s pseudo-device attach.
    unsafe { if_clone_attach(&WG_CLONER) };

    pool_init(
        &WG_AIP_POOL,
        size_of::<WgAip>(),
        0,
        IPL_NET,
        0,
        "wgaip",
        None,
    );
    pool_init(
        &WG_PEER_POOL,
        size_of::<WgPeer>(),
        0,
        IPL_NET,
        0,
        "wgpeer",
        None,
    );
    pool_init(
        &WG_RATELIMIT_POOL,
        size_of::<RatelimitEntry>(),
        0,
        IPL_NET,
        0,
        "wgratelimit",
        None,
    );
}

const _: () = {
    assert!(size_of::<WgAipIo>() == 24);
    assert!(offset_of!(WgAipIo, a_addr) == 8);
    assert!(size_of::<WgPeerIo>() == 208);
    assert!(offset_of!(WgPeerIo, p_endpoint) == 76);
    assert!(offset_of!(WgPeerIo, p_txbytes) == 104);
    assert!(offset_of!(WgPeerIo, p_last_handshake) == 120);
    assert!(offset_of!(WgPeerIo, p_description) == 136);
    assert!(offset_of!(WgPeerIo, p_aips_count) == 200);
    assert!(size_of::<WgInterfaceIo>() == 80);
    assert!(offset_of!(WgInterfaceIo, i_port) == 2);
    assert!(offset_of!(WgInterfaceIo, i_peers_count) == 72);
    assert!(size_of::<WgDataIo>() == 32);
    assert!(size_of::<WgPktInitiation>() == 148);
    assert!(size_of::<WgPktResponse>() == 92);
    assert!(size_of::<WgPktCookie>() == 64);
    assert!(size_of::<WgPktData>() == 16);
    assert!(size_of::<WgEndpoint>() == SOCKADDR_IN6_LEN + IN6_PKTINFO_LEN);
    assert!(SOCKADDR_IN6_LEN == 28 && IN6_PKTINFO_LEN == 20 && IN6_ADDR_LEN == 16);
    assert!(size_of::<SockaddrIn>() <= SOCKADDR_IN6_LEN);
    assert!(SOCKADDR_IN6_LEN <= size_of::<SockaddrStorage>());
    assert!(size_of::<WgTag>() <= PACKET_TAG_MAXSIZE);
    assert!(size_of::<MTag>().is_multiple_of(align_of::<WgTag>()));
    assert!(align_of::<WgTag>() <= align_of::<MTag>());
    assert!(offset_of!(WgAip, a_node) == 0);
    assert!(offset_of!(WgSoftc, sc_if) == 0);
};

#[cfg(test)]
mod tests;
