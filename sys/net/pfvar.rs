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
 * Copyright (c) 2001 Daniel Hartmeier
 * Copyright (c) 2002 - 2013 Henning Brauer <henning@openbsd.org>
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 *    - Redistributions of source code must retain the above copyright
 *      notice, this list of conditions and the following disclaimer.
 *    - Redistributions in binary form must reproduce the above
 *      copyright notice, this list of conditions and the following
 *      disclaimer in the documentation and/or other materials provided
 *      with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 * "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 * LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
 * FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
 * COPYRIGHT HOLDERS OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
 * BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 * CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
 * ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 *
 */
/* </LICENSES> */

/* <CODE> */
//! `<net/pfvar.h>`: the packet filter's shared types, constants and ioctl interface, see
//! `pf(4)` and `pfctl(8)`.
//!
//! Upstream: sys/net/pfvar.h @ 3ce1f3f79392
//!
//! The header is both pf's kernel interface and the ABI `pfctl(8)`, `pflogd(8)`, `tcpdump(8)`
//! and `authpf(8)` are compiled against: every structure an ioctl carries keeps the C's layout
//! to the byte (`#[repr(C)]`, the holes the C compiler leaves written as `_pad` members, sizes
//! and offsets pinned by the `layout` test against what clang computes for the C header on
//! amd64 and arm64, which agree).
//!
//! Several of those structures are also the kernel's own objects: a `struct pf_rule` is both
//! the `rule` of a `struct pfioc_rule` and an entry of a ruleset, a `struct pfi_kif` both a
//! node of the interface tree and what `DIOCIGETIFACES` copies out. They keep one type, as in
//! C. Their members that pf changes after the object is published are `Cell`s (`Cell<T>` has
//! `T`'s layout), and their pointer members are raw `Cell<*const T>` words, because a copy
//! that comes from user space holds whatever bits the program wrote there. The accessors that
//! follow a pointer member (`PfRule::kif`, `PfRulePtr::ptr`, `PfAddrWrap::tbl`, ...) take
//! `&'static self`: only the kernel's objects (pool items and the statics) are `'static`, and
//! their pointer members are null or point at live objects, which is what pf's code
//! guarantees by building every kernel object through `pf_rule_copyin`, `pf_pool_copyin` and
//! the like, which copy scalar members only (as the C's do). A user copy is a local or a
//! `Box`, so it cannot reach those accessors.
//!
//! Such structures are not `Copy`, so they cannot be `AbiPod`; they implement [`PfAbi`]
//! instead, the same promise (`#[repr(C)]`, no implicit padding, every bit pattern valid) for
//! types with `Cell`s, and are moved between user space and the kernel by [`pf_abi_read`],
//! [`pf_abi_write`], [`pf_abi_copyin`] and [`pf_abi_copyout`].
//!
//! ## Deviations
//! - The members of the unions `struct pf_addr` (`v4`, `v6`, `addr16`, `addr32`), `pfra_u`
//!   (`pfra_ip4addr`, `pfra_ip6addr`) and `union pfsockaddr_union` (`sa`, `sin`, `sin6`) are
//!   accessor methods over the union's bytes.
//! - The `PF_AEQ`, `PF_ANEQ`, `PF_AZERO`, `PF_MISMATCHAW`, `PF_POOL_DYNTYPE`,
//!   `PF_OSFP_PACK`/`UNPACK`, `PF_OSFP_ENTRY_EQ`, `REASON_SET`, `DPFPRINTF` and
//!   `pf_state_counter_hton`/`ntoh` macros are functions (`DPFPRINTF` a macro, `dpfprintf!`).
//!   `PF_MISMATCHAW` calls into `pf.c`, `pf_table.c` and `pf_if.c`, so it lives in
//!   `net/pf.rs` as `pf_mismatchaw`.
//! - The `PF_REF_*` wrappers over `refcnt(9)` are not repeated: callers use `refcnt_*`.
//! - The kernel prototypes at the end of the header are the functions themselves, in the
//!   modules of their `.c` files (`net/pf.rs`, `net/pf_ioctl.rs`, `net/pf_table.rs`, ...).
//! - `pfr_kentry`'s union `u` of one member is flattened: `PfrKentry` is `struct _pfr_kentry`
//!   and the route and cost variants embed it first (`#[repr(C)]`), as the C's overlay does.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::{MaybeUninit, size_of};
use core::ptr;

use alloc::boxed::Box;

use crate::kassert;
use crate::machine::copy::{AbiPod, copyin, copyout};
use crate::net::if_::IFNAMSIZ;
use crate::net::if_var::{IfgGroup, Ifnet};
use crate::net::radix::{RadixNode, RadixNodeHead};
use crate::net::route::RTLABEL_LEN;
use crate::netinet::in_::{InAddr, SockaddrIn};
use crate::netinet::tcp_fsm::TCP_NSTATES;
use crate::netinet6::in6::{In6Addr, SockaddrIn6};
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::queue::{SlistEntry, SlistHead, TailqEntry, TailqHead};
use crate::sys::socket::Sockaddr;
use crate::sys::syslimits::PATH_MAX;
use crate::sys::time::Timeval;
use crate::sys::timeout::Timeout;
use crate::sys::tree::{RbEntry, RbHead};
use crate::sys::types::{Pid, SaFamily, Time, Uid};

/// `PF_TCPS_PROXY_SRC`: synproxy state, talking to the client.
pub const PF_TCPS_PROXY_SRC: u8 = TCP_NSTATES as u8;
/// `PF_TCPS_PROXY_DST`: synproxy state, talking to the server.
pub const PF_TCPS_PROXY_DST: u8 = TCP_NSTATES as u8 + 1;

/// `PF_MD5_DIGEST_LENGTH`: the ruleset checksum's length.
pub const PF_MD5_DIGEST_LENGTH: usize = 16;

/// `PF_INOUT`: both directions.
pub const PF_INOUT: u8 = 0;
/// `PF_IN`.
pub const PF_IN: u8 = 1;
/// `PF_OUT`.
pub const PF_OUT: u8 = 2;
/// `PF_FWD`: forwarded (`ip_output` of a routed packet).
pub const PF_FWD: u8 = 3;

/// `PF_PASS`.
pub const PF_PASS: u8 = 0;
/// `PF_DROP`.
pub const PF_DROP: u8 = 1;
/// `PF_SCRUB`.
pub const PF_SCRUB: u8 = 2;
/// `PF_NOSCRUB`.
pub const PF_NOSCRUB: u8 = 3;
/// `PF_NAT`.
pub const PF_NAT: u8 = 4;
/// `PF_NONAT`.
pub const PF_NONAT: u8 = 5;
/// `PF_BINAT`.
pub const PF_BINAT: u8 = 6;
/// `PF_NOBINAT`.
pub const PF_NOBINAT: u8 = 7;
/// `PF_RDR`.
pub const PF_RDR: u8 = 8;
/// `PF_NORDR`.
pub const PF_NORDR: u8 = 9;
/// `PF_SYNPROXY_DROP`.
pub const PF_SYNPROXY_DROP: u8 = 10;
/// `PF_DEFER`.
pub const PF_DEFER: u8 = 11;
/// `PF_MATCH`.
pub const PF_MATCH: u8 = 12;
/// `PF_DIVERT`.
pub const PF_DIVERT: u8 = 13;
/// `PF_RT`.
pub const PF_RT: u8 = 14;
/// `PF_AFRT`.
pub const PF_AFRT: u8 = 15;

/// `PF_TRANS_RULESET`: a `pfioc_trans_e` for a ruleset.
pub const PF_TRANS_RULESET: i32 = 0;
/// `PF_TRANS_TABLE`: a `pfioc_trans_e` for the tables.
pub const PF_TRANS_TABLE: i32 = 1;

/// `PF_OP_NONE`.
pub const PF_OP_NONE: u8 = 0;
/// `PF_OP_IRG`: inside the range, exclusive (`><`).
pub const PF_OP_IRG: u8 = 1;
/// `PF_OP_EQ`.
pub const PF_OP_EQ: u8 = 2;
/// `PF_OP_NE`.
pub const PF_OP_NE: u8 = 3;
/// `PF_OP_LT`.
pub const PF_OP_LT: u8 = 4;
/// `PF_OP_LE`.
pub const PF_OP_LE: u8 = 5;
/// `PF_OP_GT`.
pub const PF_OP_GT: u8 = 6;
/// `PF_OP_GE`.
pub const PF_OP_GE: u8 = 7;
/// `PF_OP_XRG`: outside the range (`<>`).
pub const PF_OP_XRG: u8 = 8;
/// `PF_OP_RRG`: inside the range, inclusive (`:`).
pub const PF_OP_RRG: u8 = 9;

/// `PF_CHANGE_NONE`.
pub const PF_CHANGE_NONE: u32 = 0;
/// `PF_CHANGE_ADD_HEAD`.
pub const PF_CHANGE_ADD_HEAD: u32 = 1;
/// `PF_CHANGE_ADD_TAIL`.
pub const PF_CHANGE_ADD_TAIL: u32 = 2;
/// `PF_CHANGE_ADD_BEFORE`.
pub const PF_CHANGE_ADD_BEFORE: u32 = 3;
/// `PF_CHANGE_ADD_AFTER`.
pub const PF_CHANGE_ADD_AFTER: u32 = 4;
/// `PF_CHANGE_REMOVE`.
pub const PF_CHANGE_REMOVE: u32 = 5;
/// `PF_CHANGE_GET_TICKET`.
pub const PF_CHANGE_GET_TICKET: u32 = 6;

/// `PF_GET_NONE`.
pub const PF_GET_NONE: u32 = 0;
/// `PF_GET_CLR_CNTR`: clear the rule's counters while reading it.
pub const PF_GET_CLR_CNTR: u32 = 1;

/// `PF_SK_WIRE`: the state key as seen on the wire.
pub const PF_SK_WIRE: usize = 0;
/// `PF_SK_STACK`: the state key as seen by the stack.
pub const PF_SK_STACK: usize = 1;
/// `PF_SK_BOTH`.
pub const PF_SK_BOTH: usize = 2;

/// `PF_PEER_SRC`.
pub const PF_PEER_SRC: i32 = 0;
/// `PF_PEER_DST`.
pub const PF_PEER_DST: i32 = 1;
/// `PF_PEER_BOTH`.
pub const PF_PEER_BOTH: i32 = 2;

// PFTM_*: real indices into pf_rule.timeout[] come before PFTM_MAX, special cases
// afterwards. See pf_state_expires().
/// `PFTM_TCP_FIRST_PACKET`.
pub const PFTM_TCP_FIRST_PACKET: usize = 0;
/// `PFTM_TCP_OPENING`.
pub const PFTM_TCP_OPENING: usize = 1;
/// `PFTM_TCP_ESTABLISHED`.
pub const PFTM_TCP_ESTABLISHED: usize = 2;
/// `PFTM_TCP_CLOSING`.
pub const PFTM_TCP_CLOSING: usize = 3;
/// `PFTM_TCP_FIN_WAIT`.
pub const PFTM_TCP_FIN_WAIT: usize = 4;
/// `PFTM_TCP_CLOSED`.
pub const PFTM_TCP_CLOSED: usize = 5;
/// `PFTM_UDP_FIRST_PACKET`.
pub const PFTM_UDP_FIRST_PACKET: usize = 6;
/// `PFTM_UDP_SINGLE`.
pub const PFTM_UDP_SINGLE: usize = 7;
/// `PFTM_UDP_MULTIPLE`.
pub const PFTM_UDP_MULTIPLE: usize = 8;
/// `PFTM_ICMP_FIRST_PACKET`.
pub const PFTM_ICMP_FIRST_PACKET: usize = 9;
/// `PFTM_ICMP_ERROR_REPLY`.
pub const PFTM_ICMP_ERROR_REPLY: usize = 10;
/// `PFTM_OTHER_FIRST_PACKET`.
pub const PFTM_OTHER_FIRST_PACKET: usize = 11;
/// `PFTM_OTHER_SINGLE`.
pub const PFTM_OTHER_SINGLE: usize = 12;
/// `PFTM_OTHER_MULTIPLE`.
pub const PFTM_OTHER_MULTIPLE: usize = 13;
/// `PFTM_FRAG`.
pub const PFTM_FRAG: usize = 14;
/// `PFTM_INTERVAL`.
pub const PFTM_INTERVAL: usize = 15;
/// `PFTM_ADAPTIVE_START`.
pub const PFTM_ADAPTIVE_START: usize = 16;
/// `PFTM_ADAPTIVE_END`.
pub const PFTM_ADAPTIVE_END: usize = 17;
/// `PFTM_SRC_NODE`.
pub const PFTM_SRC_NODE: usize = 18;
/// `PFTM_TS_DIFF`.
pub const PFTM_TS_DIFF: usize = 19;
/// `PFTM_MAX`: the number of real timeouts.
pub const PFTM_MAX: usize = 20;
/// `PFTM_PURGE`: the state is to be purged.
pub const PFTM_PURGE: usize = 21;
/// `PFTM_UNLINKED`: the state is unlinked from the trees.
pub const PFTM_UNLINKED: usize = 22;

/// `PFTM_TCP_FIRST_PACKET_VAL`: first TCP packet.
pub const PFTM_TCP_FIRST_PACKET_VAL: u32 = 120;
/// `PFTM_TCP_OPENING_VAL`: no response yet.
pub const PFTM_TCP_OPENING_VAL: u32 = 30;
/// `PFTM_TCP_ESTABLISHED_VAL`: established.
pub const PFTM_TCP_ESTABLISHED_VAL: u32 = 24 * 60 * 60;
/// `PFTM_TCP_CLOSING_VAL`: half closed.
pub const PFTM_TCP_CLOSING_VAL: u32 = 15 * 60;
/// `PFTM_TCP_FIN_WAIT_VAL`: got both FINs.
pub const PFTM_TCP_FIN_WAIT_VAL: u32 = 45;
/// `PFTM_TCP_CLOSED_VAL`: got a RST.
pub const PFTM_TCP_CLOSED_VAL: u32 = 90;
/// `PFTM_UDP_FIRST_PACKET_VAL`: first UDP packet.
pub const PFTM_UDP_FIRST_PACKET_VAL: u32 = 60;
/// `PFTM_UDP_SINGLE_VAL`: unidirectional.
pub const PFTM_UDP_SINGLE_VAL: u32 = 30;
/// `PFTM_UDP_MULTIPLE_VAL`: bidirectional.
pub const PFTM_UDP_MULTIPLE_VAL: u32 = 60;
/// `PFTM_ICMP_FIRST_PACKET_VAL`: first ICMP packet.
pub const PFTM_ICMP_FIRST_PACKET_VAL: u32 = 20;
/// `PFTM_ICMP_ERROR_REPLY_VAL`: got error response.
pub const PFTM_ICMP_ERROR_REPLY_VAL: u32 = 10;
/// `PFTM_OTHER_FIRST_PACKET_VAL`: first packet.
pub const PFTM_OTHER_FIRST_PACKET_VAL: u32 = 60;
/// `PFTM_OTHER_SINGLE_VAL`: unidirectional.
pub const PFTM_OTHER_SINGLE_VAL: u32 = 30;
/// `PFTM_OTHER_MULTIPLE_VAL`: bidirectional.
pub const PFTM_OTHER_MULTIPLE_VAL: u32 = 60;
/// `PFTM_FRAG_VAL`: fragment expire.
pub const PFTM_FRAG_VAL: u32 = 60;
/// `PFTM_INTERVAL_VAL`: expire interval.
pub const PFTM_INTERVAL_VAL: u32 = 10;
/// `PFTM_SRC_NODE_VAL`: source tracking.
pub const PFTM_SRC_NODE_VAL: u32 = 0;
/// `PFTM_TS_DIFF_VAL`: allowed TS diff.
pub const PFTM_TS_DIFF_VAL: u32 = 30;

/// `PF_FRAG_STALE`: for each connection (combination of proto, src, dst, af) the number of
/// fragments is limited. Over the `PFTM_FRAG` interval the average rate must be less than
/// `PF_FRAG_STALE` fragments per second; otherwise older fragments are considered stale and
/// are dropped.
pub const PF_FRAG_STALE: u32 = 200;

/// `PF_FRAG_ENTRY_POINTS`: limit the length of the fragment queue traversal; remember search
/// entry points based on the fragment offset.
pub const PF_FRAG_ENTRY_POINTS: usize = 16;

/// `PF_FRAG_ENTRY_LIMIT`: the number of entries in the fragment queue must be limited to
/// avoid DoS by linear searching. Instead of a global limit, use a limit per entry point; for
/// large packets these sum up.
pub const PF_FRAG_ENTRY_LIMIT: u8 = 64;

/// `PF_NOPFROUTE`.
pub const PF_NOPFROUTE: u8 = 0;
/// `PF_ROUTETO`.
pub const PF_ROUTETO: u8 = 1;
/// `PF_DUPTO`.
pub const PF_DUPTO: u8 = 2;
/// `PF_REPLYTO`.
pub const PF_REPLYTO: u8 = 3;

/// `PF_LIMIT_STATES`.
pub const PF_LIMIT_STATES: usize = 0;
/// `PF_LIMIT_SRC_NODES`.
pub const PF_LIMIT_SRC_NODES: usize = 1;
/// `PF_LIMIT_FRAGS`.
pub const PF_LIMIT_FRAGS: usize = 2;
/// `PF_LIMIT_TABLES`.
pub const PF_LIMIT_TABLES: usize = 3;
/// `PF_LIMIT_TABLE_ENTRIES`.
pub const PF_LIMIT_TABLE_ENTRIES: usize = 4;
/// `PF_LIMIT_PKTDELAY_PKTS`.
pub const PF_LIMIT_PKTDELAY_PKTS: usize = 5;
/// `PF_LIMIT_ANCHORS`.
pub const PF_LIMIT_ANCHORS: usize = 6;
/// `PF_LIMIT_MAX`.
pub const PF_LIMIT_MAX: usize = 7;

/// `PF_POOL_IDMASK`.
pub const PF_POOL_IDMASK: u8 = 0x0f;
/// `PF_POOL_NONE`.
pub const PF_POOL_NONE: u8 = 0;
/// `PF_POOL_BITMASK`.
pub const PF_POOL_BITMASK: u8 = 1;
/// `PF_POOL_RANDOM`.
pub const PF_POOL_RANDOM: u8 = 2;
/// `PF_POOL_SRCHASH`.
pub const PF_POOL_SRCHASH: u8 = 3;
/// `PF_POOL_ROUNDROBIN`.
pub const PF_POOL_ROUNDROBIN: u8 = 4;
/// `PF_POOL_LEASTSTATES`.
pub const PF_POOL_LEASTSTATES: u8 = 5;

/// `PF_ADDR_ADDRMASK`.
pub const PF_ADDR_ADDRMASK: u8 = 0;
/// `PF_ADDR_NOROUTE`.
pub const PF_ADDR_NOROUTE: u8 = 1;
/// `PF_ADDR_DYNIFTL`.
pub const PF_ADDR_DYNIFTL: u8 = 2;
/// `PF_ADDR_TABLE`.
pub const PF_ADDR_TABLE: u8 = 3;
/// `PF_ADDR_RTLABEL`.
pub const PF_ADDR_RTLABEL: u8 = 4;
/// `PF_ADDR_URPFFAILED`.
pub const PF_ADDR_URPFFAILED: u8 = 5;
/// `PF_ADDR_RANGE`.
pub const PF_ADDR_RANGE: u8 = 6;
/// `PF_ADDR_NONE`.
pub const PF_ADDR_NONE: u8 = 7;

/// `PF_POOL_TYPEMASK`.
pub const PF_POOL_TYPEMASK: u8 = 0x0f;
/// `PF_POOL_STICKYADDR`.
pub const PF_POOL_STICKYADDR: u8 = 0x20;
/// `PF_WSCALE_FLAG`.
pub const PF_WSCALE_FLAG: u8 = 0x80;
/// `PF_WSCALE_MASK`.
pub const PF_WSCALE_MASK: u8 = 0x0f;

/// `PF_POOL_DYNTYPE(_o)`: the pool type keeps per-pool state.
pub const fn pf_pool_dyntype(o: u8) -> bool {
    let t = o & PF_POOL_TYPEMASK;
    t == PF_POOL_ROUNDROBIN
        || t == PF_POOL_LEASTSTATES
        || t == PF_POOL_RANDOM
        || t == PF_POOL_SRCHASH
}

/// `PF_LOG`.
pub const PF_LOG: u8 = 0x01;
/// `PF_LOG_ALL`.
pub const PF_LOG_ALL: u8 = 0x02;
/// `PF_LOG_USER`.
pub const PF_LOG_USER: u8 = 0x04;
/// `PF_LOG_FORCE`.
pub const PF_LOG_FORCE: u8 = 0x08;
/// `PF_LOG_MATCHES`.
pub const PF_LOG_MATCHES: u8 = 0x10;

/// `struct pf_addr`: a 128-bit address, IPv4 in the first word. The union of `v4`, `v6`,
/// `addr8`, `addr16` and `addr32` is its 16 bytes; the views are methods. The bytes are in
/// network order, as in C, so `addr16(i)`/`addr32(i)` are the native-endian reading of the
/// bytes, exactly what the C's union members read. The C type is 4-byte aligned; this one is
/// byte aligned (so that the `__packed` `pfsync_state` may contain it and a header in an mbuf
/// may be read as one), and the structures that embed it name the holes the C alignment
/// leaves, which the `layout` test checks.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct PfAddr {
    /// `addr8`.
    pub addr8: [u8; 16],
}

impl PfAddr {
    /// The all-zero address.
    pub const fn zeroed() -> Self {
        Self { addr8: [0; 16] }
    }

    /// `addr32[i]`.
    pub const fn addr32(&self, i: usize) -> u32 {
        let b = &self.addr8;
        u32::from_ne_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]])
    }

    /// `addr32[i] = v`.
    pub fn set_addr32(&mut self, i: usize, v: u32) {
        self.addr8[4 * i..4 * i + 4].copy_from_slice(&v.to_ne_bytes());
    }

    /// `addr16[i]`.
    pub const fn addr16(&self, i: usize) -> u16 {
        let b = &self.addr8;
        u16::from_ne_bytes([b[2 * i], b[2 * i + 1]])
    }

    /// `addr16[i] = v`.
    pub fn set_addr16(&mut self, i: usize, v: u16) {
        self.addr8[2 * i..2 * i + 2].copy_from_slice(&v.to_ne_bytes());
    }

    /// `v4`: the IPv4 address in the first word.
    pub const fn v4(&self) -> InAddr {
        InAddr {
            s_addr: self.addr32(0),
        }
    }

    /// `v4 = a`.
    pub fn set_v4(&mut self, a: InAddr) {
        self.set_addr32(0, a.s_addr);
    }

    /// An address whose first word is `a` and the rest zero (`pf_addr` of an IPv4 address).
    pub fn from_v4(a: InAddr) -> Self {
        let mut p = Self::zeroed();
        p.set_v4(a);
        p
    }

    /// `v6`: the IPv6 address, all 16 bytes.
    pub const fn v6(&self) -> In6Addr {
        In6Addr::new(self.addr8)
    }

    /// `v6 = a`.
    pub fn set_v6(&mut self, a: In6Addr) {
        self.addr8 = a.s6_addr;
    }

    /// The `pf_addr` of an IPv6 address.
    pub const fn from_v6(a: In6Addr) -> Self {
        Self { addr8: a.s6_addr }
    }
}

/// `PF_TABLE_NAME_SIZE`.
pub const PF_TABLE_NAME_SIZE: usize = 32;

/// `PFI_AFLAG_NETWORK`.
pub const PFI_AFLAG_NETWORK: u8 = 0x01;
/// `PFI_AFLAG_BROADCAST`.
pub const PFI_AFLAG_BROADCAST: u8 = 0x02;
/// `PFI_AFLAG_PEER`.
pub const PFI_AFLAG_PEER: u8 = 0x04;
/// `PFI_AFLAG_MODEMASK`.
pub const PFI_AFLAG_MODEMASK: u8 = 0x07;
/// `PFI_AFLAG_NOALIAS`.
pub const PFI_AFLAG_NOALIAS: u8 = 0x08;

/// `struct pf_addr_wrap`'s member `v`: an address and mask, an interface name, a table name,
/// a route label name or a route label id, by `type`. Stored as its 32 bytes, read through
/// methods named after the members.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct PfAddrWrapV {
    bytes: [u8; 32],
}

impl PfAddrWrapV {
    /// `v.a.addr`.
    pub fn addr(&self) -> PfAddr {
        let mut a = PfAddr::zeroed();
        a.addr8.copy_from_slice(&self.bytes[..16]);
        a
    }

    /// `v.a.addr = a`.
    pub fn set_addr(&mut self, a: &PfAddr) {
        self.bytes[..16].copy_from_slice(&a.addr8);
    }

    /// `v.a.mask`.
    pub fn mask(&self) -> PfAddr {
        let mut a = PfAddr::zeroed();
        a.addr8.copy_from_slice(&self.bytes[16..]);
        a
    }

    /// `v.a.mask = a`.
    pub fn set_mask(&mut self, a: &PfAddr) {
        self.bytes[16..].copy_from_slice(&a.addr8);
    }

    /// `v.ifname`.
    pub fn ifname(&self) -> &[u8] {
        &self.bytes[..IFNAMSIZ]
    }

    /// `v.ifname`, writable.
    pub fn ifname_mut(&mut self) -> &mut [u8] {
        &mut self.bytes[..IFNAMSIZ]
    }

    /// `v.tblname`.
    pub fn tblname(&self) -> &[u8] {
        &self.bytes[..PF_TABLE_NAME_SIZE]
    }

    /// `v.tblname`, writable.
    pub fn tblname_mut(&mut self) -> &mut [u8] {
        &mut self.bytes[..PF_TABLE_NAME_SIZE]
    }

    /// `v.rtlabelname`.
    pub fn rtlabelname(&self) -> &[u8] {
        &self.bytes[..RTLABEL_LEN]
    }

    /// `v.rtlabelname`, writable.
    pub fn rtlabelname_mut(&mut self) -> &mut [u8] {
        &mut self.bytes[..RTLABEL_LEN]
    }

    /// `v.rtlabel`.
    pub fn rtlabel(&self) -> u32 {
        u32::from_ne_bytes([self.bytes[0], self.bytes[1], self.bytes[2], self.bytes[3]])
    }

    /// `v.rtlabel = l`.
    pub fn set_rtlabel(&mut self, l: u32) {
        self.bytes[..4].copy_from_slice(&l.to_ne_bytes());
    }
}

/// `struct pf_addr_wrap`: an address operand of a rule or a pool.
#[repr(C)]
pub struct PfAddrWrap {
    /// `v`: the operand, by `type`.
    pub v: Cell<PfAddrWrapV>,
    /// `p`: `struct pfi_dynaddr *dyn` (`PF_ADDR_DYNIFTL`), `struct pfr_ktable *tbl`
    /// (`PF_ADDR_TABLE`) in the kernel; `int dyncnt`/`tblcnt` in a copy for user space.
    pub p: Cell<usize>,
    /// `type`: `PF_ADDR_*`.
    pub type_: Cell<u8>,
    /// `iflags`: `PFI_AFLAG_*`.
    pub iflags: Cell<u8>,
    /// A hole the C compiler leaves.
    pub _pad: [u8; 6],
}

impl PfAddrWrap {
    /// An all-zero operand.
    pub const fn zeroed() -> Self {
        Self {
            v: Cell::new(PfAddrWrapV { bytes: [0; 32] }),
            p: Cell::new(0),
            type_: Cell::new(0),
            iflags: Cell::new(0),
            _pad: [0; 6],
        }
    }

    /// `p.dyn`: the dynamic address of a kernel operand of type `PF_ADDR_DYNIFTL`.
    pub fn dyn_(&'static self) -> Option<&'static PfiDynaddr> {
        // SAFETY: a `'static` operand belongs to a kernel rule or pool, whose `p` is null or
        // was set by `pfi_dynaddr_setup` to a `pfi_addr_pl` item that lives until
        // `pfi_dynaddr_remove` clears it (the module's invariant).
        unsafe { (self.p.get() as *const PfiDynaddr).as_ref() }
    }

    /// `p.dyn = d`.
    pub fn set_dyn(&self, d: Option<&'static PfiDynaddr>) {
        self.p.set(d.map_or(0, |d| ptr::from_ref(d) as usize));
    }

    /// `p.tbl`: the table of a kernel operand of type `PF_ADDR_TABLE`.
    pub fn tbl(&'static self) -> Option<&'static PfrKtable> {
        // SAFETY: as for `dyn_`: set by `pf_tbladdr_setup` to a table that lives until
        // `pf_tbladdr_remove` detaches it.
        unsafe { (self.p.get() as *const PfrKtable).as_ref() }
    }

    /// `p.tbl = t`.
    pub fn set_tbl(&self, t: Option<&'static PfrKtable>) {
        self.p.set(t.map_or(0, |t| ptr::from_ref(t) as usize));
    }

    /// `p.dyncnt` / `p.tblcnt`: the count a copy for user space carries.
    pub fn cnt(&self) -> i32 {
        self.p.get() as i32
    }

    /// `p.dyncnt = n` / `p.tblcnt = n`.
    pub fn set_cnt(&self, n: i32) {
        // The C stores an int in the union, leaving the other bytes of the pointer as they
        // were; storing the sign-extended word gives user space the same int.
        self.p.set(n as isize as usize);
    }

    /// A copy of the scalar members (`v`, `type`, `iflags`), `p` cleared.
    pub fn copy_scalars(&self) -> Self {
        let n = Self::zeroed();
        n.v.set(self.v.get());
        n.type_.set(self.type_.get());
        n.iflags.set(self.iflags.get());
        n
    }
}

/// `struct pfi_dynaddr`: an interface's addresses as an operand (`(ifname)`), kept up to date
/// by the interface's address hook.
pub struct PfiDynaddr {
    /// `entry`: on the kif's `pfik_dynaddrs`.
    pub entry: TailqEntry<PfiDynaddr>,
    /// `pfid_addr4`.
    pub pfid_addr4: Cell<PfAddr>,
    /// `pfid_mask4`.
    pub pfid_mask4: Cell<PfAddr>,
    /// `pfid_addr6`.
    pub pfid_addr6: Cell<PfAddr>,
    /// `pfid_mask6`.
    pub pfid_mask6: Cell<PfAddr>,
    /// `pfid_kt`: the table holding the addresses.
    pub pfid_kt: Cell<Option<&'static PfrKtable>>,
    /// `pfid_kif`.
    pub pfid_kif: Cell<Option<&'static PfiKif>>,
    /// `pfid_hook_cookie`.
    pub pfid_hook_cookie: Cell<*mut c_void>,
    /// `pfid_net`: mask or 128.
    pub pfid_net: Cell<i32>,
    /// `pfid_acnt4`: address count IPv4.
    pub pfid_acnt4: Cell<i32>,
    /// `pfid_acnt6`: address count IPv6.
    pub pfid_acnt6: Cell<i32>,
    /// `pfid_af`: rule af.
    pub pfid_af: Cell<SaFamily>,
    /// `pfid_iflags`: `PFI_AFLAG_*`.
    pub pfid_iflags: Cell<u8>,
}

// SAFETY: pf's objects change under the net lock and `pf_lock`, as in C.
unsafe impl Sync for PfiDynaddr {}

crate::queue_adapter!(
    /// `TAILQ_HEAD(, pfi_dynaddr)`: a kif's dynamic addresses.
    pub PfiDynaddrList: PfiDynaddr, entry => TailqEntry<PfiDynaddr>
);

/// `PF_DEBUGNAME`.
pub const PF_DEBUGNAME: &str = "pf: ";

/// `DPFPRINTF(n, format, ...)`: logs at level `n` when `pf_status.debug >= n`.
#[macro_export]
macro_rules! dpfprintf {
    ($n:expr, $($arg:tt)*) => {{
        if $crate::net::pf::PF_STATUS.debug.get() >= ($n) as u32 {
            $crate::kern::subr_prf::log(($n) as i32, format_args!("{}", $crate::net::pfvar::PF_DEBUGNAME));
            $crate::kern::subr_prf::addlog(format_args!($($arg)*));
            $crate::kern::subr_prf::addlog(format_args!("\n"));
        }
    }};
}

/// `PF_AEQ(a, b, c)`: the addresses are equal in family `c`.
pub fn pf_aeq(a: &PfAddr, b: &PfAddr, c: SaFamily) -> bool {
    use crate::sys::socket::{AF_INET, AF_INET6};
    (c == AF_INET && a.addr32(0) == b.addr32(0))
        || (c == AF_INET6
            && a.addr32(3) == b.addr32(3)
            && a.addr32(2) == b.addr32(2)
            && a.addr32(1) == b.addr32(1)
            && a.addr32(0) == b.addr32(0))
}

/// `PF_ANEQ(a, b, c)`: the addresses differ in family `c`.
pub fn pf_aneq(a: &PfAddr, b: &PfAddr, c: SaFamily) -> bool {
    use crate::sys::socket::{AF_INET, AF_INET6};
    (c == AF_INET && a.addr32(0) != b.addr32(0))
        || (c == AF_INET6
            && (a.addr32(3) != b.addr32(3)
                || a.addr32(2) != b.addr32(2)
                || a.addr32(1) != b.addr32(1)
                || a.addr32(0) != b.addr32(0)))
}

/// `PF_AZERO(a, c)`: the address is zero in family `c`.
pub fn pf_azero(a: &PfAddr, c: SaFamily) -> bool {
    use crate::sys::socket::{AF_INET, AF_INET6};
    (c == AF_INET && a.addr32(0) == 0)
        || (c == AF_INET6
            && a.addr32(0) == 0
            && a.addr32(1) == 0
            && a.addr32(2) == 0
            && a.addr32(3) == 0)
}

/// `struct pf_rule_uid`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PfRuleUid {
    /// `uid[2]`.
    pub uid: [Uid; 2],
    /// `op`: `PF_OP_*`.
    pub op: u8,
    /// A hole the C compiler leaves.
    pub _pad: [u8; 3],
}

/// `struct pf_rule_gid`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PfRuleGid {
    /// `gid[2]` (declared `uid_t` in the C).
    pub gid: [Uid; 2],
    /// `op`: `PF_OP_*`.
    pub op: u8,
    /// A hole the C compiler leaves.
    pub _pad: [u8; 3],
}

/// `struct pf_rule_addr`: an address operand with its port range.
#[repr(C)]
pub struct PfRuleAddr {
    /// `addr`.
    pub addr: PfAddrWrap,
    /// `port[2]`, network order.
    pub port: [Cell<u16>; 2],
    /// `neg`.
    pub neg: Cell<u8>,
    /// `port_op`: `PF_OP_*`.
    pub port_op: Cell<u8>,
    /// `weight`.
    pub weight: Cell<u16>,
}

impl PfRuleAddr {
    /// An all-zero operand.
    pub const fn zeroed() -> Self {
        Self {
            addr: PfAddrWrap::zeroed(),
            port: [Cell::new(0), Cell::new(0)],
            neg: Cell::new(0),
            port_op: Cell::new(0),
            weight: Cell::new(0),
        }
    }

    /// A copy of the scalar members (the wrap's `p` cleared).
    pub fn copy_scalars(&self) -> Self {
        Self {
            addr: self.addr.copy_scalars(),
            port: [Cell::new(self.port[0].get()), Cell::new(self.port[1].get())],
            neg: Cell::new(self.neg.get()),
            port_op: Cell::new(self.port_op.get()),
            weight: Cell::new(self.weight.get()),
        }
    }
}

/// `PF_THRESHOLD_MULT`.
pub const PF_THRESHOLD_MULT: u32 = 1000;
/// `PF_THRESHOLD_MAX`.
pub const PF_THRESHOLD_MAX: u32 = 0xffff_ffff / PF_THRESHOLD_MULT;

/// `struct pf_threshold`: a rate limit, `limit` events per `seconds`.
#[repr(C)]
#[derive(Default)]
pub struct PfThreshold {
    /// `limit`.
    pub limit: Cell<u32>,
    /// `seconds`.
    pub seconds: Cell<u32>,
    /// `count`.
    pub count: Cell<u32>,
    /// `last`.
    pub last: Cell<u32>,
}

impl PfThreshold {
    /// An all-zero threshold.
    pub const fn zeroed() -> Self {
        Self {
            limit: Cell::new(0),
            seconds: Cell::new(0),
            count: Cell::new(0),
            last: Cell::new(0),
        }
    }
}

/// `struct pf_poolhashkey`: a 128-bit hash key; `key8`, `key16`, `key32` are views of the
/// bytes.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct PfPoolhashkey {
    /// `key8`.
    pub key8: [u8; 16],
}

impl PfPoolhashkey {
    /// `key32[i]`.
    pub const fn key32(&self, i: usize) -> u32 {
        let b = &self.key8;
        u32::from_ne_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]])
    }
}

/// `struct pf_pool`: an address pool (`nat-to`, `rdr-to`, `route-to`).
#[repr(C)]
pub struct PfPool {
    /// `addr`.
    pub addr: PfAddrWrap,
    /// `key`.
    pub key: Cell<PfPoolhashkey>,
    /// `counter`: the round-robin position.
    pub counter: Cell<PfAddr>,
    /// `ifname`.
    pub ifname: Cell<[u8; IFNAMSIZ]>,
    /// `kif`.
    pub kif: Cell<*const PfiKif>,
    /// `tblidx`.
    pub tblidx: Cell<i32>,
    /// A hole the C compiler leaves.
    pub _pad0: [u8; 4],
    /// `states`.
    pub states: Cell<u64>,
    /// `curweight`.
    pub curweight: Cell<i32>,
    /// `weight`.
    pub weight: Cell<u16>,
    /// `proxy_port[2]`.
    pub proxy_port: [Cell<u16>; 2],
    /// `port_op`.
    pub port_op: Cell<u8>,
    /// `opts`: `PF_POOL_*`.
    pub opts: Cell<u8>,
    /// A hole the C compiler leaves.
    pub _pad1: [u8; 4],
}

impl PfPool {
    /// An all-zero pool.
    pub const fn zeroed() -> Self {
        Self {
            addr: PfAddrWrap::zeroed(),
            key: Cell::new(PfPoolhashkey { key8: [0; 16] }),
            counter: Cell::new(PfAddr::zeroed()),
            ifname: Cell::new([0; IFNAMSIZ]),
            kif: Cell::new(ptr::null()),
            tblidx: Cell::new(0),
            _pad0: [0; 4],
            states: Cell::new(0),
            curweight: Cell::new(0),
            weight: Cell::new(0),
            proxy_port: [Cell::new(0), Cell::new(0)],
            port_op: Cell::new(0),
            opts: Cell::new(0),
            _pad1: [0; 4],
        }
    }

    /// `kif`: the pool's interface, of a kernel pool.
    pub fn kif(&'static self) -> Option<&'static PfiKif> {
        // SAFETY: a `'static` pool belongs to a kernel rule; its `kif` is null or was set by
        // `pf_kif_setup` to a kif it holds a reference on (the module's invariant).
        unsafe { self.kif.get().as_ref() }
    }

    /// `kif = k`.
    pub fn set_kif(&self, k: Option<&'static PfiKif>) {
        self.kif.set(k.map_or(ptr::null(), ptr::from_ref));
    }
}

/// `pf_osfp_t`: a packed operating system description for fingerprinting.
pub type PfOsfp = u32;
/// `PF_OSFP_ANY`.
pub const PF_OSFP_ANY: PfOsfp = 0;
/// `PF_OSFP_UNKNOWN`.
pub const PF_OSFP_UNKNOWN: PfOsfp = -1i32 as u32;
/// `PF_OSFP_NOMATCH`.
pub const PF_OSFP_NOMATCH: PfOsfp = -2i32 as u32;

/// `PF_OSFP_EXPANDED`: expanded entry.
pub const PF_OSFP_EXPANDED: i32 = 0x001;
/// `PF_OSFP_GENERIC`: generic signature.
pub const PF_OSFP_GENERIC: i32 = 0x002;
/// `PF_OSFP_NODETAIL`: no p0f details.
pub const PF_OSFP_NODETAIL: i32 = 0x004;
/// `PF_OSFP_LEN`.
pub const PF_OSFP_LEN: usize = 32;

/// `struct pf_osfp_entry`: one operating system a fingerprint matches.
#[repr(C)]
#[derive(Default)]
pub struct PfOsfpEntry {
    /// `fp_entry`.
    pub fp_entry: SlistEntry<PfOsfpEntry>,
    /// `fp_os`.
    pub fp_os: PfOsfp,
    /// `fp_enflags`: `PF_OSFP_EXPANDED`, ...
    pub fp_enflags: i32,
    /// `fp_class_nm`.
    pub fp_class_nm: [u8; PF_OSFP_LEN],
    /// `fp_version_nm`.
    pub fp_version_nm: [u8; PF_OSFP_LEN],
    /// `fp_subtype_nm`.
    pub fp_subtype_nm: [u8; PF_OSFP_LEN],
}

/// `PF_OSFP_ENTRY_EQ(a, b)`.
pub fn pf_osfp_entry_eq(a: &PfOsfpEntry, b: &PfOsfpEntry) -> bool {
    a.fp_os == b.fp_os
        && a.fp_class_nm == b.fp_class_nm
        && a.fp_version_nm == b.fp_version_nm
        && a.fp_subtype_nm == b.fp_subtype_nm
}

/// `_FP_RESERVED_BIT`: for the special negative values.
pub const FP_RESERVED_BIT: u32 = 1;
/// `_FP_UNUSED_BITS`.
pub const FP_UNUSED_BITS: u32 = 1;
/// `_FP_CLASS_BITS`: OS class (Windows, Linux).
pub const FP_CLASS_BITS: u32 = 10;
/// `_FP_VERSION_BITS`: OS version (95, 98, NT, 2.4.54, 3.2).
pub const FP_VERSION_BITS: u32 = 10;
/// `_FP_SUBTYPE_BITS`: patch level (NT SP4, SP3, ECN patch).
pub const FP_SUBTYPE_BITS: u32 = 10;

/// `PF_OSFP_UNPACK(osfp, class, version, subtype)`: returns `(class, version, subtype)`.
pub const fn pf_osfp_unpack(osfp: PfOsfp) -> (u32, u32, u32) {
    (
        (osfp >> (FP_VERSION_BITS + FP_SUBTYPE_BITS)) & ((1 << FP_CLASS_BITS) - 1),
        (osfp >> FP_SUBTYPE_BITS) & ((1 << FP_VERSION_BITS) - 1),
        osfp & ((1 << FP_SUBTYPE_BITS) - 1),
    )
}

/// `PF_OSFP_PACK(osfp, class, version, subtype)`: returns `osfp`.
pub const fn pf_osfp_pack(class: u32, version: u32, subtype: u32) -> PfOsfp {
    ((class & ((1 << FP_CLASS_BITS) - 1)) << (FP_VERSION_BITS + FP_SUBTYPE_BITS))
        | ((version & ((1 << FP_VERSION_BITS) - 1)) << FP_SUBTYPE_BITS)
        | (subtype & ((1 << FP_SUBTYPE_BITS) - 1))
}

/// `pf_tcpopts_t`: packed TCP options.
pub type PfTcpopts = u64;

crate::queue_adapter!(
    /// `SLIST_HEAD(pf_osfp_enlist, pf_osfp_entry)`: the operating systems of a fingerprint.
    pub PfOsfpEnlist: PfOsfpEntry, fp_entry => SlistEntry<PfOsfpEntry>
);

/// `struct pf_os_fingerprint`: the fingerprint of an OS's TCP SYN packet.
#[derive(Default)]
pub struct PfOsFingerprint {
    /// `fp_oses`: list of matches.
    pub fp_oses: SlistHead<PfOsfpEnlist>,
    /// `fp_tcpopts`: packed TCP options.
    pub fp_tcpopts: PfTcpopts,
    /// `fp_wsize`: TCP window size.
    pub fp_wsize: u16,
    /// `fp_psize`: `ip->ip_len`.
    pub fp_psize: u16,
    /// `fp_mss`: TCP MSS.
    pub fp_mss: u16,
    /// `fp_flags`: `PF_OSFP_WSIZE_MOD`, ...
    pub fp_flags: u16,
    /// `fp_optcnt`: TCP option count.
    pub fp_optcnt: u8,
    /// `fp_wscale`: TCP window scaling.
    pub fp_wscale: u8,
    /// `fp_ttl`: IPv4 TTL.
    pub fp_ttl: u8,
    /// `fp_next`.
    pub fp_next: SlistEntry<PfOsFingerprint>,
}

/// `PF_OSFP_WSIZE_MOD`: window modulus.
pub const PF_OSFP_WSIZE_MOD: u16 = 0x0001;
/// `PF_OSFP_WSIZE_DC`: window don't care.
pub const PF_OSFP_WSIZE_DC: u16 = 0x0002;
/// `PF_OSFP_WSIZE_MSS`: window multiple of MSS.
pub const PF_OSFP_WSIZE_MSS: u16 = 0x0004;
/// `PF_OSFP_WSIZE_MTU`: window multiple of MTU.
pub const PF_OSFP_WSIZE_MTU: u16 = 0x0008;
/// `PF_OSFP_PSIZE_MOD`: packet size modulus.
pub const PF_OSFP_PSIZE_MOD: u16 = 0x0010;
/// `PF_OSFP_PSIZE_DC`: packet size don't care.
pub const PF_OSFP_PSIZE_DC: u16 = 0x0020;
/// `PF_OSFP_WSCALE`: TCP window scaling.
pub const PF_OSFP_WSCALE: u16 = 0x0040;
/// `PF_OSFP_WSCALE_MOD`: TCP window scale modulus.
pub const PF_OSFP_WSCALE_MOD: u16 = 0x0080;
/// `PF_OSFP_WSCALE_DC`: TCP window scale don't care.
pub const PF_OSFP_WSCALE_DC: u16 = 0x0100;
/// `PF_OSFP_MSS`: TCP MSS.
pub const PF_OSFP_MSS: u16 = 0x0200;
/// `PF_OSFP_MSS_MOD`: TCP MSS modulus.
pub const PF_OSFP_MSS_MOD: u16 = 0x0400;
/// `PF_OSFP_MSS_DC`: TCP MSS don't care.
pub const PF_OSFP_MSS_DC: u16 = 0x0800;
/// `PF_OSFP_DF`: IPv4 don't fragment bit.
pub const PF_OSFP_DF: u16 = 0x1000;
/// `PF_OSFP_TS0`: zero timestamp.
pub const PF_OSFP_TS0: u16 = 0x2000;
/// `PF_OSFP_INET6`: IPv6.
pub const PF_OSFP_INET6: u16 = 0x4000;
/// `PF_OSFP_MAXTTL_OFFSET`.
pub const PF_OSFP_MAXTTL_OFFSET: u8 = 40;
/// `PF_OSFP_TCPOPT_NOP`: TCP NOP option.
pub const PF_OSFP_TCPOPT_NOP: u64 = 0x0;
/// `PF_OSFP_TCPOPT_WSCALE`: TCP window scaling option.
pub const PF_OSFP_TCPOPT_WSCALE: u64 = 0x1;
/// `PF_OSFP_TCPOPT_MSS`: TCP max segment size option.
pub const PF_OSFP_TCPOPT_MSS: u64 = 0x2;
/// `PF_OSFP_TCPOPT_SACK`: TCP SACK OK option.
pub const PF_OSFP_TCPOPT_SACK: u64 = 0x3;
/// `PF_OSFP_TCPOPT_TS`: TCP timestamp option.
pub const PF_OSFP_TCPOPT_TS: u64 = 0x4;
/// `PF_OSFP_TCPOPT_BITS`: bits used by each option.
pub const PF_OSFP_TCPOPT_BITS: u32 = 3;
/// `PF_OSFP_MAX_OPTS`.
pub const PF_OSFP_MAX_OPTS: usize = (PfTcpopts::BITS / PF_OSFP_TCPOPT_BITS) as usize;

/// `struct pf_osfp_ioctl`: a fingerprint as `DIOCOSFPADD`/`DIOCOSFPGET` carry it.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PfOsfpIoctl {
    /// `fp_os`.
    pub fp_os: PfOsfpIoctlEntry,
    /// `fp_tcpopts`: packed TCP options.
    pub fp_tcpopts: PfTcpopts,
    /// `fp_wsize`: TCP window size.
    pub fp_wsize: u16,
    /// `fp_psize`: `ip->ip_len`.
    pub fp_psize: u16,
    /// `fp_mss`: TCP MSS.
    pub fp_mss: u16,
    /// `fp_flags`.
    pub fp_flags: u16,
    /// `fp_optcnt`: TCP option count.
    pub fp_optcnt: u8,
    /// `fp_wscale`: TCP window scaling.
    pub fp_wscale: u8,
    /// `fp_ttl`: IPv4 TTL.
    pub fp_ttl: u8,
    /// A hole the C compiler leaves.
    pub _pad: u8,
    /// `fp_getnum`: `DIOCOSFPGET` number.
    pub fp_getnum: i32,
}

/// The `struct pf_osfp_entry` inside a `struct pf_osfp_ioctl`: the same layout with the list
/// link as a plain word (user space neither reads nor writes it).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PfOsfpIoctlEntry {
    /// `fp_entry`.
    pub fp_entry: usize,
    /// `fp_os`.
    pub fp_os: PfOsfp,
    /// `fp_enflags`.
    pub fp_enflags: i32,
    /// `fp_class_nm`.
    pub fp_class_nm: [u8; PF_OSFP_LEN],
    /// `fp_version_nm`.
    pub fp_version_nm: [u8; PF_OSFP_LEN],
    /// `fp_subtype_nm`.
    pub fp_subtype_nm: [u8; PF_OSFP_LEN],
}

// SAFETY: integers and byte arrays, no implicit padding (`_pad` names the one hole).
unsafe impl AbiPod for PfOsfpIoctl {}

/// `struct pf_rule_actions`: what the matching rules ask of a packet.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfRuleActions {
    /// `rtableid`.
    pub rtableid: i32,
    /// `qid`.
    pub qid: u16,
    /// `pqid`.
    pub pqid: u16,
    /// `max_mss`.
    pub max_mss: u16,
    /// `flags`.
    pub flags: u16,
    /// `delay`.
    pub delay: u16,
    /// `log`.
    pub log: u8,
    /// `set_tos`.
    pub set_tos: u8,
    /// `min_ttl`.
    pub min_ttl: u8,
    /// `set_prio[2]`.
    pub set_prio: [u8; 2],
    /// `pad[1]`.
    pub pad: [u8; 1],
}

/// `union pf_rule_ptr`: a kernel rule's address, or its number in a copy for user space.
#[repr(C)]
pub struct PfRulePtr(Cell<usize>);

impl PfRulePtr {
    /// A null pointer.
    pub const fn null() -> Self {
        Self(Cell::new(0))
    }

    /// `ptr` of a kernel object.
    pub fn ptr(&'static self) -> Option<&'static PfRule> {
        // SAFETY: a `'static` rule pointer is a member of a kernel object (rule, state, source
        // node), set only by `set_ptr` to a rule that outlives it (rules are freed after the
        // states and nodes that point at them: `pf_rm_rule`'s `states_cur`/`src_nodes`
        // checks), or null.
        unsafe { (self.0.get() as *const PfRule).as_ref() }
    }

    /// `ptr = r`.
    pub fn set_ptr(&self, r: Option<&'static PfRule>) {
        self.0.set(r.map_or(0, |r| ptr::from_ref(r) as usize));
    }

    /// `nr`: the rule number of a copy for user space.
    pub fn nr(&self) -> u32 {
        self.0.get() as u32
    }

    /// `nr = n`: the C writes the low 32 bits of the union; the rest of the word is cleared.
    pub fn set_nr(&self, n: u32) {
        self.0.set(n as usize);
    }

    /// The raw word, for comparisons (`r->skip[i].ptr == s`).
    pub fn word(&self) -> usize {
        self.0.get()
    }
}

/// `PF_ANCHOR_STACK_MAX`.
pub const PF_ANCHOR_STACK_MAX: usize = 64;
/// `PF_ANCHOR_NAME_SIZE`.
pub const PF_ANCHOR_NAME_SIZE: usize = 64;
/// `PF_ANCHOR_MAXPATH`.
pub const PF_ANCHOR_MAXPATH: usize = PATH_MAX - PF_ANCHOR_NAME_SIZE - 1;
/// `PF_ANCHOR_HIWAT`.
pub const PF_ANCHOR_HIWAT: u32 = 512;
/// `PF_OPTIMIZER_TABLE_PFX`.
pub const PF_OPTIMIZER_TABLE_PFX: &[u8] = b"__automatic_";

/// `PF_LIMITER_NOMATCH`.
pub const PF_LIMITER_NOMATCH: i32 = 0;
/// `PF_LIMITER_BLOCK`.
pub const PF_LIMITER_BLOCK: i32 = 1;
/// `PF_LIMITER_DEFAULT`.
pub const PF_LIMITER_DEFAULT: i32 = PF_LIMITER_BLOCK;

/// `PF_SKIP_IFP`.
pub const PF_SKIP_IFP: usize = 0;
/// `PF_SKIP_DIR`.
pub const PF_SKIP_DIR: usize = 1;
/// `PF_SKIP_RDOM`.
pub const PF_SKIP_RDOM: usize = 2;
/// `PF_SKIP_AF`.
pub const PF_SKIP_AF: usize = 3;
/// `PF_SKIP_PROTO`.
pub const PF_SKIP_PROTO: usize = 4;
/// `PF_SKIP_SRC_ADDR`.
pub const PF_SKIP_SRC_ADDR: usize = 5;
/// `PF_SKIP_DST_ADDR`.
pub const PF_SKIP_DST_ADDR: usize = 6;
/// `PF_SKIP_SRC_PORT`.
pub const PF_SKIP_SRC_PORT: usize = 7;
/// `PF_SKIP_DST_PORT`.
pub const PF_SKIP_DST_PORT: usize = 8;
/// `PF_SKIP_COUNT`.
pub const PF_SKIP_COUNT: usize = 9;
/// `PF_RULE_LABEL_SIZE`.
pub const PF_RULE_LABEL_SIZE: usize = 64;
/// `PF_QNAME_SIZE`.
pub const PF_QNAME_SIZE: usize = 64;
/// `PF_TAG_NAME_SIZE`.
pub const PF_TAG_NAME_SIZE: usize = 64;

/// The `max_src_conn_rate` member of `struct pf_rule`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfRuleConnRate {
    /// `limit`.
    pub limit: u32,
    /// `seconds`.
    pub seconds: u32,
}

/// The `statelim`/`sourcelim` members of `struct pf_rule`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfRuleLimiter {
    /// `id`.
    pub id: u8,
    /// A hole the C compiler leaves.
    pub _pad: [u8; 3],
    /// `limiter_action`: `PF_LIMITER_*`.
    pub limiter_action: i32,
}

/// The `divert` member of `struct pf_rule`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfRuleDivert {
    /// `addr`.
    pub addr: PfAddr,
    /// `port`.
    pub port: u16,
    /// `type`: `PF_DIVERT_*`.
    pub type_: u8,
    /// A hole the C compiler leaves.
    pub _pad: u8,
}

/// `struct pf_rule`: one rule of a ruleset, and the `rule` a `struct pfioc_rule` carries.
/// Kernel rules are `pf_rule_pl` items (and the static `pf_default_rule`); see the module
/// docs for the members that are `Cell`s and the pointer members.
#[repr(C)]
pub struct PfRule {
    /// `src`.
    pub src: PfRuleAddr,
    /// `dst`.
    pub dst: PfRuleAddr,
    /// `skip[PF_SKIP_COUNT]`: the rule to jump to when a criterion fails.
    pub skip: [PfRulePtr; PF_SKIP_COUNT],
    /// `label`.
    pub label: [u8; PF_RULE_LABEL_SIZE],
    /// `ifname`.
    pub ifname: [u8; IFNAMSIZ],
    /// `rcv_ifname`.
    pub rcv_ifname: [u8; IFNAMSIZ],
    /// `qname`.
    pub qname: [u8; PF_QNAME_SIZE],
    /// `pqname`.
    pub pqname: [u8; PF_QNAME_SIZE],
    /// `tagname`.
    pub tagname: [u8; PF_TAG_NAME_SIZE],
    /// `match_tagname`.
    pub match_tagname: [u8; PF_TAG_NAME_SIZE],
    /// `overload_tblname`.
    pub overload_tblname: [u8; PF_TABLE_NAME_SIZE],
    /// `entries`: on the ruleset's queue.
    pub entries: TailqEntry<PfRule>,
    /// `nat`.
    pub nat: PfPool,
    /// `rdr`.
    pub rdr: PfPool,
    /// `route`.
    pub route: PfPool,
    /// `pktrate`.
    pub pktrate: PfThreshold,
    /// `evaluations`.
    pub evaluations: Cell<u64>,
    /// `packets[2]`.
    pub packets: [Cell<u64>; 2],
    /// `bytes[2]`.
    pub bytes: [Cell<u64>; 2],
    /// `kif`.
    pub kif: Cell<*const PfiKif>,
    /// `rcv_kif`.
    pub rcv_kif: Cell<*const PfiKif>,
    /// `anchor`.
    pub anchor: Cell<*const PfAnchor>,
    /// `overload_tbl`.
    pub overload_tbl: Cell<*const PfrKtable>,
    /// `os_fingerprint`.
    pub os_fingerprint: PfOsfp,
    /// `rtableid`.
    pub rtableid: i32,
    /// `onrdomain`.
    pub onrdomain: i32,
    /// `timeout[PFTM_MAX]`.
    pub timeout: [Cell<u32>; PFTM_MAX],
    /// `states_cur`.
    pub states_cur: Cell<u32>,
    /// `states_tot`.
    pub states_tot: Cell<u32>,
    /// `max_states`.
    pub max_states: u32,
    /// `src_nodes`.
    pub src_nodes: Cell<u32>,
    /// `max_src_nodes`.
    pub max_src_nodes: u32,
    /// `max_src_states`.
    pub max_src_states: u32,
    /// `max_src_conn`.
    pub max_src_conn: u32,
    /// `max_src_conn_rate`.
    pub max_src_conn_rate: PfRuleConnRate,
    /// `qid`.
    pub qid: u32,
    /// `pqid`.
    pub pqid: u32,
    /// `rt_listid`.
    pub rt_listid: u32,
    /// `nr`.
    pub nr: Cell<u32>,
    /// `prob`.
    pub prob: u32,
    /// `cuid`.
    pub cuid: Uid,
    /// `cpid`.
    pub cpid: Pid,
    /// `return_icmp`.
    pub return_icmp: u16,
    /// `return_icmp6`.
    pub return_icmp6: u16,
    /// `max_mss`.
    pub max_mss: u16,
    /// `tag`.
    pub tag: u16,
    /// `match_tag`.
    pub match_tag: u16,
    /// `scrub_flags`.
    pub scrub_flags: u16,
    /// `delay`.
    pub delay: u16,
    /// A hole the C compiler leaves.
    pub _pad0: [u8; 2],
    /// `uid`.
    pub uid: PfRuleUid,
    /// `gid`.
    pub gid: PfRuleGid,
    /// `rule_flag`: `PFRULE_*`.
    pub rule_flag: Cell<u32>,
    /// `action`: `PF_PASS`, ...
    pub action: u8,
    /// `direction`.
    pub direction: u8,
    /// `log`.
    pub log: u8,
    /// `logif`.
    pub logif: u8,
    /// `quick`.
    pub quick: u8,
    /// `ifnot`.
    pub ifnot: u8,
    /// `match_tag_not`.
    pub match_tag_not: u8,
    /// `keep_state`: `PF_STATE_*`.
    pub keep_state: u8,
    /// `af`.
    pub af: SaFamily,
    /// `proto`.
    pub proto: u8,
    /// `type`: aux. value 256 is legit.
    pub type_: u16,
    /// `code`: aux. value 256 is legit.
    pub code: u16,
    /// `flags`.
    pub flags: u8,
    /// `flagset`.
    pub flagset: u8,
    /// `min_ttl`.
    pub min_ttl: u8,
    /// `allow_opts`.
    pub allow_opts: u8,
    /// `rt`.
    pub rt: u8,
    /// `return_ttl`.
    pub return_ttl: u8,
    /// `tos`.
    pub tos: u8,
    /// `set_tos`.
    pub set_tos: u8,
    /// `anchor_relative`.
    pub anchor_relative: Cell<u8>,
    /// `anchor_wildcard`.
    pub anchor_wildcard: Cell<u8>,
    /// `flush`: `PF_FLUSH`, `PF_FLUSH_GLOBAL`.
    pub flush: u8,
    /// `prio`.
    pub prio: u8,
    /// `set_prio[2]`.
    pub set_prio: [u8; 2],
    /// `naf`.
    pub naf: SaFamily,
    /// `rcvifnot`.
    pub rcvifnot: u8,
    /// A hole the C compiler leaves.
    pub _pad1: [u8; 2],
    /// `statelim`.
    pub statelim: PfRuleLimiter,
    /// `sourcelim`.
    pub sourcelim: PfRuleLimiter,
    /// `divert`.
    pub divert: PfRuleDivert,
    /// A hole the C compiler leaves.
    pub _pad2: [u8; 4],
    /// `exptime`.
    pub exptime: Cell<Time>,
}

// SAFETY: the counters and links change under the net lock and `pf_lock`, as in C.
unsafe impl Sync for PfRule {}

// SAFETY: `#[repr(C)]` integers, byte arrays, `Cell`s of those and raw pointer words; the
// holes the C compiler leaves are the `_pad` members (the `layout` test pins the offsets),
// and any bit pattern is a valid value.
unsafe impl PfAbi for PfRule {}

impl PfRule {
    /// An all-zero rule (`bzero`).
    pub const fn zeroed() -> Self {
        // SAFETY: `PfRule: PfAbi`: every bit pattern, the zero one included, is valid.
        unsafe { MaybeUninit::zeroed().assume_init() }
    }

    /// `kif` of a kernel rule.
    pub fn kif(&'static self) -> Option<&'static PfiKif> {
        // SAFETY: the module's invariant for `'static` objects: set by `pf_kif_setup` to a
        // referenced kif, or null.
        unsafe { self.kif.get().as_ref() }
    }

    /// `kif = k`.
    pub fn set_kif(&self, k: Option<&'static PfiKif>) {
        self.kif.set(k.map_or(ptr::null(), ptr::from_ref));
    }

    /// `rcv_kif` of a kernel rule.
    pub fn rcv_kif(&'static self) -> Option<&'static PfiKif> {
        // SAFETY: as for `kif`.
        unsafe { self.rcv_kif.get().as_ref() }
    }

    /// `rcv_kif = k`.
    pub fn set_rcv_kif(&self, k: Option<&'static PfiKif>) {
        self.rcv_kif.set(k.map_or(ptr::null(), ptr::from_ref));
    }

    /// `anchor` of a kernel rule.
    pub fn anchor(&'static self) -> Option<&'static PfAnchor> {
        // SAFETY: set by `pf_anchor_setup` to an anchor whose `refcnt` the rule holds, cleared
        // by `pf_remove_anchor`.
        unsafe { self.anchor.get().as_ref() }
    }

    /// `anchor = a`.
    pub fn set_anchor(&self, a: Option<&'static PfAnchor>) {
        self.anchor.set(a.map_or(ptr::null(), ptr::from_ref));
    }

    /// `overload_tbl` of a kernel rule.
    pub fn overload_tbl(&'static self) -> Option<&'static PfrKtable> {
        // SAFETY: set by `DIOCADDRULE` through `pfr_attach_table`, detached by `pf_rm_rule`.
        unsafe { self.overload_tbl.get().as_ref() }
    }

    /// `overload_tbl = t`.
    pub fn set_overload_tbl(&self, t: Option<&'static PfrKtable>) {
        self.overload_tbl.set(t.map_or(ptr::null(), ptr::from_ref));
    }

    /// `timeout[i]`.
    pub fn timeout(&self, i: usize) -> u32 {
        self.timeout[i].get()
    }
}

/// `PF_STATE_NORMAL`.
pub const PF_STATE_NORMAL: u8 = 0x1;
/// `PF_STATE_MODULATE`.
pub const PF_STATE_MODULATE: u8 = 0x2;
/// `PF_STATE_SYNPROXY`.
pub const PF_STATE_SYNPROXY: u8 = 0x3;

/// `PF_FLUSH`.
pub const PF_FLUSH: u8 = 0x01;
/// `PF_FLUSH_GLOBAL`.
pub const PF_FLUSH_GLOBAL: u8 = 0x02;

/// `PFRULE_DROP`.
pub const PFRULE_DROP: u32 = 0x0000;
/// `PFRULE_RETURNRST`.
pub const PFRULE_RETURNRST: u32 = 0x0001;
/// `PFRULE_FRAGMENT`.
pub const PFRULE_FRAGMENT: u32 = 0x0002;
/// `PFRULE_RETURNICMP`.
pub const PFRULE_RETURNICMP: u32 = 0x0004;
/// `PFRULE_RETURN`.
pub const PFRULE_RETURN: u32 = 0x0008;
/// `PFRULE_NOSYNC`.
pub const PFRULE_NOSYNC: u32 = 0x0010;
/// `PFRULE_SRCTRACK`: track source states.
pub const PFRULE_SRCTRACK: u32 = 0x0020;
/// `PFRULE_RULESRCTRACK`: per rule.
pub const PFRULE_RULESRCTRACK: u32 = 0x0040;
/// `PFRULE_SETDELAY`.
pub const PFRULE_SETDELAY: u32 = 0x0080;
/// `PFRULE_IFBOUND`: if-bound.
pub const PFRULE_IFBOUND: u32 = 0x0001_0000;
/// `PFRULE_STATESLOPPY`: sloppy state tracking.
pub const PFRULE_STATESLOPPY: u32 = 0x0002_0000;
/// `PFRULE_PFLOW`.
pub const PFRULE_PFLOW: u32 = 0x0004_0000;
/// `PFRULE_ONCE`: one shot rule.
pub const PFRULE_ONCE: u32 = 0x0010_0000;
/// `PFRULE_AFTO`: af-to rule.
pub const PFRULE_AFTO: u32 = 0x0020_0000;
/// `PFRULE_EXPIRED`: one shot rule hit by a packet.
pub const PFRULE_EXPIRED: u32 = 0x0040_0000;

/// `PFSTATE_HIWAT`: default state table size.
pub const PFSTATE_HIWAT: u32 = 100_000;
/// `PFSTATE_ADAPT_START`: default adaptive timeout start.
pub const PFSTATE_ADAPT_START: u32 = 60_000;
/// `PFSTATE_ADAPT_END`: default adaptive timeout end.
pub const PFSTATE_ADAPT_END: u32 = 120_000;
/// `PF_PKTDELAY_MAXPKTS`: max number of packets held in the delay queue.
pub const PF_PKTDELAY_MAXPKTS: u32 = 10_000;

/// `struct pf_rule_item`: a rule on a state's `match_rules`.
pub struct PfRuleItem {
    /// `entry`.
    pub entry: SlistEntry<PfRuleItem>,
    /// `r`: set when the item is made, before it is linked.
    pub r: Cell<Option<&'static PfRule>>,
}

impl PfRuleItem {
    /// `r`.
    #[allow(clippy::panic)] // an item is linked only after `r_set`, as in C
    pub fn r(&self) -> &'static PfRule {
        match self.r.get() {
            Some(r) => r,
            None => panic!("pf_rule_item without a rule"),
        }
    }

    /// `r = rule`.
    pub fn r_set(&self, rule: &'static PfRule) {
        self.r.set(Some(rule));
    }
}

crate::queue_adapter!(
    /// `SLIST_HEAD(pf_rule_slist, pf_rule_item)`.
    pub PfRuleSlist: PfRuleItem, entry => SlistEntry<PfRuleItem>
);

/// `enum pf_sn_types`.
pub type PfSnTypes = usize;
/// `PF_SN_NONE`.
pub const PF_SN_NONE: PfSnTypes = 0;
/// `PF_SN_NAT`.
pub const PF_SN_NAT: PfSnTypes = 1;
/// `PF_SN_RDR`.
pub const PF_SN_RDR: PfSnTypes = 2;
/// `PF_SN_ROUTE`.
pub const PF_SN_ROUTE: PfSnTypes = 3;
/// `PF_SN_MAX`.
pub const PF_SN_MAX: PfSnTypes = 4;

/// `struct pf_src_node`: a source tracking node; also what `DIOCGETSRCNODES` copies out.
#[repr(C)]
pub struct PfSrcNode {
    /// `entry`: in `tree_src_tracking`.
    pub entry: RbEntry<PfSrcNode>,
    /// `addr`.
    pub addr: Cell<PfAddr>,
    /// `raddr`.
    pub raddr: Cell<PfAddr>,
    /// `rule`.
    pub rule: PfRulePtr,
    /// `kif`.
    pub kif: Cell<*const PfiKif>,
    /// `bytes[2]`.
    pub bytes: [Cell<u64>; 2],
    /// `packets[2]`.
    pub packets: [Cell<u64>; 2],
    /// `states`.
    pub states: Cell<u32>,
    /// `conn`.
    pub conn: Cell<u32>,
    /// `conn_rate`.
    pub conn_rate: PfThreshold,
    /// `creation`.
    pub creation: Cell<i32>,
    /// `expire`.
    pub expire: Cell<i32>,
    /// `af`.
    pub af: Cell<SaFamily>,
    /// `naf`.
    pub naf: Cell<SaFamily>,
    /// `type`: `PF_SN_*`.
    pub type_: Cell<u8>,
    /// A hole the C compiler leaves.
    pub _pad: [u8; 5],
}

// SAFETY: changed under the net lock and `pf_lock`, as in C.
unsafe impl Sync for PfSrcNode {}

// SAFETY: `#[repr(C)]`, integers, `Cell`s of integers and raw words, the hole named `_pad`;
// the tree entry is three raw pointers and a colour word with its padding: `RbEntry` is
// `#[repr(C)]` of `Cell<*const>` words and a `u32`, whose 4 trailing bytes are covered by
// `pf_abi_read`'s zeroing and never read as a value.
unsafe impl PfAbi for PfSrcNode {}

impl PfSrcNode {
    /// `kif` of a kernel node.
    pub fn kif(&'static self) -> Option<&'static PfiKif> {
        // SAFETY: set by `pf_insert_src_node` to a referenced kif, or null.
        unsafe { self.kif.get().as_ref() }
    }

    /// `kif = k`.
    pub fn set_kif(&self, k: Option<&'static PfiKif>) {
        self.kif.set(k.map_or(ptr::null(), ptr::from_ref));
    }
}

/// `struct pf_sn_item`: a source node on a state's `src_nodes`.
pub struct PfSnItem {
    /// `next`.
    pub next: SlistEntry<PfSnItem>,
    /// `sn`: set when the item is made, before it is linked.
    pub sn: Cell<Option<&'static PfSrcNode>>,
}

impl PfSnItem {
    /// `sn`.
    #[allow(clippy::panic)] // an item is linked only after `sn_set`, as in C
    pub fn sn(&self) -> &'static PfSrcNode {
        match self.sn.get() {
            Some(sn) => sn,
            None => panic!("pf_sn_item without a source node"),
        }
    }

    /// `sn = n`.
    pub fn sn_set(&self, n: &'static PfSrcNode) {
        self.sn.set(Some(n));
    }
}

crate::queue_adapter!(
    /// `SLIST_HEAD(pf_sn_head, pf_sn_item)`.
    pub PfSnHead: PfSnItem, next => SlistEntry<PfSnItem>
);

/// `PFSNODE_HIWAT`: default source node table size.
pub const PFSNODE_HIWAT: u32 = 10_000;

/// `struct pf_state_scrub`: the normalizer's per-peer state.
#[derive(Default)]
pub struct PfStateScrub {
    /// `pfss_last`: time received last packet.
    pub pfss_last: Cell<Timeval>,
    /// `pfss_tsecr`: last echoed timestamp.
    pub pfss_tsecr: Cell<u32>,
    /// `pfss_tsval`: largest timestamp.
    pub pfss_tsval: Cell<u32>,
    /// `pfss_tsval0`: original timestamp.
    pub pfss_tsval0: Cell<u32>,
    /// `pfss_flags`: `PFSS_*`.
    pub pfss_flags: Cell<u16>,
    /// `pfss_ttl`: stashed TTL.
    pub pfss_ttl: Cell<u8>,
    /// `pad`.
    pub pad: Cell<u8>,
    /// `pfss_ts_mod`: timestamp modulation.
    pub pfss_ts_mod: Cell<u32>,
}

/// `PFSS_TIMESTAMP`: modulate timestamp.
pub const PFSS_TIMESTAMP: u16 = 0x0001;
/// `PFSS_PAWS`: stricter PAWS checks.
pub const PFSS_PAWS: u16 = 0x0010;
/// `PFSS_PAWS_IDLED`: was idle too long, no PAWS.
pub const PFSS_PAWS_IDLED: u16 = 0x0020;
/// `PFSS_DATA_TS`: timestamp on data packets.
pub const PFSS_DATA_TS: u16 = 0x0040;
/// `PFSS_DATA_NOTS`: no timestamp on data packets.
pub const PFSS_DATA_NOTS: u16 = 0x0080;

/// `struct pf_state_host`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfStateHost {
    /// `addr`.
    pub addr: PfAddr,
    /// `port`.
    pub port: u16,
    /// `pad`.
    pub pad: u16,
}

/// `struct pf_state_peer`: one side of a state.
#[derive(Default)]
pub struct PfStatePeer {
    /// `scrub`: the state is scrubbed.
    pub scrub: Cell<Option<&'static PfStateScrub>>,
    /// `seqlo`: max sequence number sent.
    pub seqlo: Cell<u32>,
    /// `seqhi`: max the other end ACKd + win.
    pub seqhi: Cell<u32>,
    /// `seqdiff`: sequence number modulator.
    pub seqdiff: Cell<u32>,
    /// `max_win`: largest window (pre scaling).
    pub max_win: Cell<u16>,
    /// `mss`: maximum segment size option.
    pub mss: Cell<u16>,
    /// `state`: active state level.
    pub state: Cell<u8>,
    /// `wscale`: window scaling factor.
    pub wscale: Cell<u8>,
    /// `tcp_est`: did we reach TCPS_ESTABLISHED.
    pub tcp_est: Cell<u8>,
    /// `pad[1]`.
    pub pad: Cell<u8>,
}

/// `struct pf_state_key_cmp`: the lookup key of a state key (`RB_FIND` with a partial
/// object). Kept in sync with `struct pf_state_key`'s first members.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfStateKeyCmp {
    /// `addr[2]`.
    pub addr: [PfAddr; 2],
    /// `port[2]`.
    pub port: [u16; 2],
    /// `rdomain`.
    pub rdomain: u16,
    /// `hash`.
    pub hash: u16,
    /// `af`.
    pub af: SaFamily,
    /// `proto`.
    pub proto: u8,
    /// A hole the C compiler leaves.
    pub _pad: [u8; 2],
}

/// `struct pf_state_cmp`: the lookup key of a state by id.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfStateCmp {
    /// `id`.
    pub id: u64,
    /// `creatorid`.
    pub creatorid: u32,
    /// `direction`.
    pub direction: u8,
    /// `pad[3]`.
    pub pad: [u8; 3],
}

/// `PFSTATE_ALLOWOPTS`.
pub const PFSTATE_ALLOWOPTS: u16 = 0x0001;
/// `PFSTATE_SLOPPY`.
pub const PFSTATE_SLOPPY: u16 = 0x0002;
/// `PFSTATE_PFLOW`.
pub const PFSTATE_PFLOW: u16 = 0x0004;
/// `PFSTATE_NOSYNC`.
pub const PFSTATE_NOSYNC: u16 = 0x0008;
/// `PFSTATE_ACK`.
pub const PFSTATE_ACK: u16 = 0x0010;
/// `PFSTATE_NODF`.
pub const PFSTATE_NODF: u16 = 0x0020;
/// `PFSTATE_SETTOS`.
pub const PFSTATE_SETTOS: u16 = 0x0040;
/// `PFSTATE_RANDOMID`.
pub const PFSTATE_RANDOMID: u16 = 0x0080;
/// `PFSTATE_SCRUB_TCP`.
pub const PFSTATE_SCRUB_TCP: u16 = 0x0100;
/// `PFSTATE_SETPRIO`.
pub const PFSTATE_SETPRIO: u16 = 0x0200;
/// `PFSTATE_INP_UNLINKED`.
pub const PFSTATE_INP_UNLINKED: u16 = 0x0400;
/// `PFSTATE_SCRUBMASK`.
pub const PFSTATE_SCRUBMASK: u16 = PFSTATE_NODF | PFSTATE_RANDOMID | PFSTATE_SCRUB_TCP;
/// `PFSTATE_SETMASK`.
pub const PFSTATE_SETMASK: u16 = PFSTATE_SETTOS | PFSTATE_SETPRIO;

/// `struct pfsync_state_scrub` (`__packed`).
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct PfsyncStateScrub {
    /// `pfss_flags`.
    pub pfss_flags: u16,
    /// `pfss_ttl`: stashed TTL.
    pub pfss_ttl: u8,
    /// `scrub_flag`.
    pub scrub_flag: u8,
    /// `pfss_ts_mod`: timestamp modulation.
    pub pfss_ts_mod: u32,
}

/// `PFSYNC_SCRUB_FLAG_VALID`.
pub const PFSYNC_SCRUB_FLAG_VALID: u8 = 0x01;

/// `struct pfsync_state_peer` (`__packed`): a state peer in network order.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct PfsyncStatePeer {
    /// `scrub`: the state is scrubbed.
    pub scrub: PfsyncStateScrub,
    /// `seqlo`: max sequence number sent.
    pub seqlo: u32,
    /// `seqhi`: max the other end ACKd + win.
    pub seqhi: u32,
    /// `seqdiff`: sequence number modulator.
    pub seqdiff: u32,
    /// `max_win`: largest window (pre scaling).
    pub max_win: u16,
    /// `mss`: maximum segment size option.
    pub mss: u16,
    /// `state`: active state level.
    pub state: u8,
    /// `wscale`: window scaling factor.
    pub wscale: u8,
    /// `pad[6]`.
    pub pad: [u8; 6],
}

/// `struct pfsync_state_key`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PfsyncStateKey {
    /// `addr[2]`.
    pub addr: [PfAddr; 2],
    /// `port[2]`.
    pub port: [u16; 2],
    /// `rdomain`.
    pub rdomain: u16,
    /// `af`.
    pub af: SaFamily,
    /// `pad`.
    pub pad: u8,
}

/// `struct pfsync_state` (`__packed`): a state as pfsync(4) and `DIOCGETSTATE(S)` carry it.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct PfsyncState {
    /// `id`.
    pub id: u64,
    /// `ifname`.
    pub ifname: [u8; IFNAMSIZ],
    /// `key[2]`.
    pub key: [PfsyncStateKey; 2],
    /// `src`.
    pub src: PfsyncStatePeer,
    /// `dst`.
    pub dst: PfsyncStatePeer,
    /// `rt_addr`.
    pub rt_addr: PfAddr,
    /// `rule`.
    pub rule: u32,
    /// `anchor`.
    pub anchor: u32,
    /// `nat_rule`.
    pub nat_rule: u32,
    /// `creation`.
    pub creation: u32,
    /// `expire`.
    pub expire: u32,
    /// `packets[2][2]`.
    pub packets: [[u32; 2]; 2],
    /// `bytes[2][2]`.
    pub bytes: [[u32; 2]; 2],
    /// `creatorid`.
    pub creatorid: u32,
    /// `rtableid[2]`.
    pub rtableid: [i32; 2],
    /// `max_mss`.
    pub max_mss: u16,
    /// `af`.
    pub af: SaFamily,
    /// `proto`.
    pub proto: u8,
    /// `direction`.
    pub direction: u8,
    /// `log`.
    pub log: u8,
    /// `rt`.
    pub rt: u8,
    /// `timeout`.
    pub timeout: u8,
    /// `sync_flags`.
    pub sync_flags: u8,
    /// `updates`.
    pub updates: u8,
    /// `min_ttl`.
    pub min_ttl: u8,
    /// `set_tos`.
    pub set_tos: u8,
    /// `state_flags`.
    pub state_flags: u16,
    /// `set_prio[2]`.
    pub set_prio: [u8; 2],
}

// SAFETY: packed integers and arrays of integers: no padding, any bit pattern valid.
unsafe impl AbiPod for PfsyncState {}

/// `PFSYNC_FLAG_SRCNODE`.
pub const PFSYNC_FLAG_SRCNODE: u8 = 0x04;
/// `PFSYNC_FLAG_NATSRCNODE`.
pub const PFSYNC_FLAG_NATSRCNODE: u8 = 0x08;

/// `pf_state_counter_hton(s, d)`: a 64-bit counter as two network-order words.
pub const fn pf_state_counter_hton(s: u64) -> [u32; 2] {
    [((s >> 32) as u32).to_be(), (s as u32).to_be()]
}

/// `pf_state_counter_from_pfsync(s)`.
pub const fn pf_state_counter_from_pfsync(s: [u32; 2]) -> u64 {
    ((s[0] as u64) << 32) | s[1] as u64
}

/// `pf_state_counter_ntoh(s, d)`.
pub const fn pf_state_counter_ntoh(s: [u32; 2]) -> u64 {
    ((u32::from_be(s[0]) as u64) << 32) + u32::from_be(s[1]) as u64
}

crate::queue_adapter!(
    /// `TAILQ_HEAD(pf_rulequeue, pf_rule)`.
    pub PfRulequeue: PfRule, entries => TailqEntry<PfRule>
);

/// The `active`/`inactive` members of a ruleset's `rules`.
#[derive(Default)]
pub struct PfRulesetRules {
    /// `ptr`: which of `queues[]` (the index replaces the C's pointer into the same struct).
    pub ptr: Cell<usize>,
    /// `rcount`.
    pub rcount: Cell<u32>,
    /// `version`.
    pub version: Cell<u32>,
    /// `open`.
    pub open: Cell<i32>,
}

/// `struct pf_ruleset`: the active and inactive rule queues of an anchor.
pub struct PfRuleset {
    /// `rules.queues[2]`.
    pub queues: [TailqHead<PfRulequeue>; 2],
    /// `rules.active`.
    pub active: PfRulesetRules,
    /// `rules.inactive`.
    pub inactive: PfRulesetRules,
    /// `anchor`: the anchor this ruleset belongs to (NULL for the main ruleset).
    pub anchor: Cell<Option<&'static PfAnchor>>,
    /// `tticket`.
    pub tticket: Cell<u32>,
    /// `tables`.
    pub tables: Cell<i32>,
    /// `topen`.
    pub topen: Cell<i32>,
}

impl PfRuleset {
    /// An empty ruleset (before `pf_init_ruleset`).
    pub const fn new() -> Self {
        Self {
            queues: [TailqHead::new(), TailqHead::new()],
            active: PfRulesetRules {
                ptr: Cell::new(0),
                rcount: Cell::new(0),
                version: Cell::new(0),
                open: Cell::new(0),
            },
            inactive: PfRulesetRules {
                ptr: Cell::new(1),
                rcount: Cell::new(0),
                version: Cell::new(0),
                open: Cell::new(0),
            },
            anchor: Cell::new(None),
            tticket: Cell::new(0),
            tables: Cell::new(0),
            topen: Cell::new(0),
        }
    }

    /// `rules.active.ptr`.
    pub fn active_ptr(&self) -> &TailqHead<PfRulequeue> {
        &self.queues[self.active.ptr.get()]
    }

    /// `rules.inactive.ptr`.
    pub fn inactive_ptr(&self) -> &TailqHead<PfRulequeue> {
        &self.queues[self.inactive.ptr.get()]
    }
}

impl Default for PfRuleset {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: changed under `pf_lock`, as in C.
unsafe impl Sync for PfRuleset {}

/// `struct pf_anchor`: a named ruleset, in the global tree and its parent's children.
pub struct PfAnchor {
    /// `entry_global`.
    pub entry_global: RbEntry<PfAnchor>,
    /// `entry_node`.
    pub entry_node: RbEntry<PfAnchor>,
    /// `parent`.
    pub parent: Cell<Option<&'static PfAnchor>>,
    /// `children`.
    pub children: RbHead<PfAnchorNode>,
    /// `name`.
    pub name: [u8; PF_ANCHOR_NAME_SIZE],
    /// `path`.
    pub path: [u8; PATH_MAX],
    /// `ruleset`.
    pub ruleset: PfRuleset,
    /// `refcnt`: anchor rules.
    pub refcnt: Cell<i32>,
    /// `match`.
    pub match_: Cell<i32>,
    /// `ref`: for transactions.
    pub ref_: crate::sys::refcnt::Refcnt,
}

// SAFETY: changed under `pf_lock`, as in C.
unsafe impl Sync for PfAnchor {}

impl PfAnchor {
    /// An empty anchor (the static `pf_main_anchor` and `pool_get(PR_ZERO)`).
    pub const fn new() -> Self {
        Self {
            entry_global: RbEntry::new(),
            entry_node: RbEntry::new(),
            parent: Cell::new(None),
            children: RbHead::new(),
            name: [0; PF_ANCHOR_NAME_SIZE],
            path: [0; PATH_MAX],
            ruleset: PfRuleset::new(),
            refcnt: Cell::new(0),
            match_: Cell::new(0),
            ref_: crate::sys::refcnt::Refcnt::new(),
        }
    }
}

impl Default for PfAnchor {
    fn default() -> Self {
        Self::new()
    }
}

/// `pf_anchor_compare`: anchors are ordered by path.
pub fn pf_anchor_compare(a: &PfAnchor, b: &PfAnchor) -> core::cmp::Ordering {
    pf_cstr(&a.path).cmp(pf_cstr(&b.path))
}

crate::tree_adapter!(
    /// `RB_HEAD(pf_anchor_global, pf_anchor)`.
    pub PfAnchorGlobal: PfAnchor, entry_global => RbEntry<PfAnchor>, pf_anchor_compare
);
crate::tree_adapter!(
    /// `RB_HEAD(pf_anchor_node, pf_anchor)`.
    pub PfAnchorNode: PfAnchor, entry_node => RbEntry<PfAnchor>, pf_anchor_compare
);

/// `PF_RESERVED_ANCHOR`.
pub const PF_RESERVED_ANCHOR: &[u8] = b"_pf";

/// `PFR_TFLAG_PERSIST`.
pub const PFR_TFLAG_PERSIST: u32 = 0x0000_0001;
/// `PFR_TFLAG_CONST`.
pub const PFR_TFLAG_CONST: u32 = 0x0000_0002;
/// `PFR_TFLAG_ACTIVE`.
pub const PFR_TFLAG_ACTIVE: u32 = 0x0000_0004;
/// `PFR_TFLAG_INACTIVE`.
pub const PFR_TFLAG_INACTIVE: u32 = 0x0000_0008;
/// `PFR_TFLAG_REFERENCED`.
pub const PFR_TFLAG_REFERENCED: u32 = 0x0000_0010;
/// `PFR_TFLAG_REFDANCHOR`.
pub const PFR_TFLAG_REFDANCHOR: u32 = 0x0000_0020;
/// `PFR_TFLAG_COUNTERS`.
pub const PFR_TFLAG_COUNTERS: u32 = 0x0000_0040;
/// `PFR_TFLAG_USRMASK`.
pub const PFR_TFLAG_USRMASK: u32 = 0x0000_0043;
/// `PFR_TFLAG_SETMASK`.
pub const PFR_TFLAG_SETMASK: u32 = 0x0000_003C;
/// `PFR_TFLAG_ALLMASK`.
pub const PFR_TFLAG_ALLMASK: u32 = 0x0000_007F;

/// `struct pfr_table`: a table's name, anchor and flags.
#[repr(C)]
pub struct PfrTable {
    /// `pfrt_anchor`.
    pub pfrt_anchor: [u8; PATH_MAX],
    /// `pfrt_name`.
    pub pfrt_name: [u8; PF_TABLE_NAME_SIZE],
    /// `pfrt_flags`: `PFR_TFLAG_*`.
    pub pfrt_flags: Cell<u32>,
    /// `pfrt_fback`: `PFR_FB_*`.
    pub pfrt_fback: Cell<u8>,
    /// A hole the C compiler leaves.
    pub _pad: [u8; 3],
}

// SAFETY: `#[repr(C)]` byte arrays and `Cell`s of integers, the hole named `_pad`.
unsafe impl PfAbi for PfrTable {}

impl PfrTable {
    /// An all-zero table description.
    pub const fn zeroed() -> Self {
        // SAFETY: `PfAbi`: every bit pattern is valid.
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
}

/// `PFR_FB_NONE`.
pub const PFR_FB_NONE: u8 = 0;
/// `PFR_FB_MATCH`.
pub const PFR_FB_MATCH: u8 = 1;
/// `PFR_FB_ADDED`.
pub const PFR_FB_ADDED: u8 = 2;
/// `PFR_FB_DELETED`.
pub const PFR_FB_DELETED: u8 = 3;
/// `PFR_FB_CHANGED`.
pub const PFR_FB_CHANGED: u8 = 4;
/// `PFR_FB_CLEARED`.
pub const PFR_FB_CLEARED: u8 = 5;
/// `PFR_FB_DUPLICATE`.
pub const PFR_FB_DUPLICATE: u8 = 6;
/// `PFR_FB_NOTMATCH`.
pub const PFR_FB_NOTMATCH: u8 = 7;
/// `PFR_FB_CONFLICT`.
pub const PFR_FB_CONFLICT: u8 = 8;
/// `PFR_FB_NOCOUNT`.
pub const PFR_FB_NOCOUNT: u8 = 9;
/// `PFR_FB_MAX`.
pub const PFR_FB_MAX: u8 = 10;

/// `struct pfr_addr`: a table address as user space passes it.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfrAddr {
    /// `pfra_u`: `pfra_ip4addr` or `pfra_ip6addr`, 16 bytes.
    pub pfra_u: PfAddr,
    /// `pfra_ifname`.
    pub pfra_ifname: [u8; IFNAMSIZ],
    /// `pfra_states`.
    pub pfra_states: u32,
    /// `pfra_weight`.
    pub pfra_weight: u16,
    /// `pfra_af`.
    pub pfra_af: u8,
    /// `pfra_net`.
    pub pfra_net: u8,
    /// `pfra_not`.
    pub pfra_not: u8,
    /// `pfra_fback`.
    pub pfra_fback: u8,
    /// `pfra_type`.
    pub pfra_type: u8,
    /// `pad[7]`.
    pub pad: [u8; 7],
    /// A hole the C compiler leaves.
    pub _pad: [u8; 2],
}

// SAFETY: integers and byte arrays, the tail hole named `_pad`.
unsafe impl AbiPod for PfrAddr {}

impl PfrAddr {
    /// `pfra_ip4addr`.
    pub fn pfra_ip4addr(&self) -> InAddr {
        self.pfra_u.v4()
    }

    /// `pfra_ip6addr`.
    pub fn pfra_ip6addr(&self) -> In6Addr {
        self.pfra_u.v6()
    }
}

/// `PFR_DIR_IN`.
pub const PFR_DIR_IN: usize = 0;
/// `PFR_DIR_OUT`.
pub const PFR_DIR_OUT: usize = 1;
/// `PFR_DIR_MAX`.
pub const PFR_DIR_MAX: usize = 2;
/// `PFR_OP_BLOCK`.
pub const PFR_OP_BLOCK: usize = 0;
/// `PFR_OP_MATCH`.
pub const PFR_OP_MATCH: usize = 1;
/// `PFR_OP_PASS`.
pub const PFR_OP_PASS: usize = 2;
/// `PFR_OP_ADDR_MAX`.
pub const PFR_OP_ADDR_MAX: usize = 3;
/// `PFR_OP_TABLE_MAX`.
pub const PFR_OP_TABLE_MAX: usize = 4;
/// `PFR_OP_XPASS`.
pub const PFR_OP_XPASS: usize = PFR_OP_ADDR_MAX;

/// `struct pfr_astats`: an address with its counters.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfrAstats {
    /// `pfras_a`.
    pub pfras_a: PfrAddr,
    /// A hole the C compiler leaves.
    pub _pad: [u8; 4],
    /// `pfras_packets[PFR_DIR_MAX][PFR_OP_ADDR_MAX]`.
    pub pfras_packets: [[u64; PFR_OP_ADDR_MAX]; PFR_DIR_MAX],
    /// `pfras_bytes[PFR_DIR_MAX][PFR_OP_ADDR_MAX]`.
    pub pfras_bytes: [[u64; PFR_OP_ADDR_MAX]; PFR_DIR_MAX],
    /// `pfras_tzero`.
    pub pfras_tzero: Time,
}

// SAFETY: integers and arrays of them, the hole named `_pad`.
unsafe impl AbiPod for PfrAstats {}

/// `PFR_REFCNT_RULE`.
pub const PFR_REFCNT_RULE: usize = 0;
/// `PFR_REFCNT_ANCHOR`.
pub const PFR_REFCNT_ANCHOR: usize = 1;
/// `PFR_REFCNT_MAX`.
pub const PFR_REFCNT_MAX: usize = 2;

/// `struct pfr_tstats`: a table with its counters.
#[repr(C)]
pub struct PfrTstats {
    /// `pfrts_t`.
    pub pfrts_t: PfrTable,
    /// `pfrts_packets[PFR_DIR_MAX][PFR_OP_TABLE_MAX]`.
    pub pfrts_packets: [[Cell<u64>; PFR_OP_TABLE_MAX]; PFR_DIR_MAX],
    /// `pfrts_bytes[PFR_DIR_MAX][PFR_OP_TABLE_MAX]`.
    pub pfrts_bytes: [[Cell<u64>; PFR_OP_TABLE_MAX]; PFR_DIR_MAX],
    /// `pfrts_match`.
    pub pfrts_match: Cell<u64>,
    /// `pfrts_nomatch`.
    pub pfrts_nomatch: Cell<u64>,
    /// `pfrts_tzero`.
    pub pfrts_tzero: Cell<Time>,
    /// `pfrts_cnt`.
    pub pfrts_cnt: Cell<i32>,
    /// `pfrts_refcnt[PFR_REFCNT_MAX]`.
    pub pfrts_refcnt: [Cell<i32>; PFR_REFCNT_MAX],
    /// A hole the C compiler leaves.
    pub _pad: [u8; 4],
}

// SAFETY: `#[repr(C)]` of a `PfAbi` table and `Cell`s of integers, the hole named `_pad`.
unsafe impl PfAbi for PfrTstats {}

/// `struct pfr_kcounters`: an entry's counters.
#[derive(Default)]
pub struct PfrKcounters {
    /// `pfrkc_packets[PFR_DIR_MAX][PFR_OP_ADDR_MAX]`.
    pub pfrkc_packets: [[Cell<u64>; PFR_OP_ADDR_MAX]; PFR_DIR_MAX],
    /// `pfrkc_bytes[PFR_DIR_MAX][PFR_OP_ADDR_MAX]`.
    pub pfrkc_bytes: [[Cell<u64>; PFR_OP_ADDR_MAX]; PFR_DIR_MAX],
    /// `states`.
    pub states: Cell<u64>,
}

/// `union pfsockaddr_union`: a `sockaddr`, a `sockaddr_in` or a `sockaddr_in6` (28 bytes).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PfsockaddrUnion {
    /// The union's bytes.
    pub bytes: [u8; 28],
}

impl PfsockaddrUnion {
    /// `sa`: the generic view, as a pointer for the radix code.
    pub fn sa(&self) -> *const Sockaddr {
        self.bytes.as_ptr().cast()
    }

    /// `sin`.
    pub fn sin(&self) -> SockaddrIn {
        // SAFETY: `SockaddrIn` is 16 bytes of integers, within the 28-byte union, read
        // unaligned.
        unsafe { ptr::read_unaligned(self.bytes.as_ptr().cast::<SockaddrIn>()) }
    }

    /// `sin = s`.
    pub fn set_sin(&mut self, s: &SockaddrIn) {
        // SAFETY: as for `sin`; the write stays within the union.
        unsafe { ptr::write_unaligned(self.bytes.as_mut_ptr().cast::<SockaddrIn>(), *s) }
    }

    /// `sin6`.
    pub fn sin6(&self) -> SockaddrIn6 {
        // SAFETY: `SockaddrIn6` is 28 bytes of integers, the whole union, read unaligned.
        unsafe { ptr::read_unaligned(self.bytes.as_ptr().cast::<SockaddrIn6>()) }
    }

    /// `sin6 = s`.
    pub fn set_sin6(&mut self, s: &SockaddrIn6) {
        // SAFETY: as for `sin6`; the write is the whole union.
        unsafe { ptr::write_unaligned(self.bytes.as_mut_ptr().cast::<SockaddrIn6>(), *s) }
    }

    /// `sa.sa_family`.
    pub fn sa_family(&self) -> SaFamily {
        self.bytes[1]
    }
}

/// `PFRKE_FLAG_NOT`.
pub const PFRKE_FLAG_NOT: u8 = 0x01;
/// `PFRKE_FLAG_MARK`.
pub const PFRKE_FLAG_MARK: u8 = 0x02;

/// `PFRKE_PLAIN`.
pub const PFRKE_PLAIN: u8 = 0;
/// `PFRKE_ROUTE`.
pub const PFRKE_ROUTE: u8 = 1;
/// `PFRKE_COST`.
pub const PFRKE_COST: u8 = 2;
/// `PFRKE_MAX`.
pub const PFRKE_MAX: u8 = 3;

/// `struct pfr_kentry` (`struct _pfr_kentry`): a table entry, two radix nodes and the key.
#[repr(C)]
pub struct PfrKentry {
    /// `pfrke_node[2]`: the radix nodes the entry is linked by.
    pub pfrke_node: [RadixNode; 2],
    /// `pfrke_sa`: the key (address).
    pub pfrke_sa: Cell<PfsockaddrUnion>,
    /// `pfrke_workq`.
    pub pfrke_workq: SlistEntry<PfrKentry>,
    /// `pfrke_ioq`.
    pub pfrke_ioq: SlistEntry<PfrKentry>,
    /// `pfrke_counters`.
    pub pfrke_counters: Cell<Option<&'static PfrKcounters>>,
    /// `pfrke_tzero`.
    pub pfrke_tzero: Cell<Time>,
    /// `pfrke_af`.
    pub pfrke_af: Cell<u8>,
    /// `pfrke_net`.
    pub pfrke_net: Cell<u8>,
    /// `pfrke_flags`: `PFRKE_FLAG_*`.
    pub pfrke_flags: Cell<u8>,
    /// `pfrke_type`: `PFRKE_*`.
    pub pfrke_type: Cell<u8>,
    /// `pfrke_fb`.
    pub pfrke_fb: Cell<u8>,
}

// SAFETY: changed under `pf_lock` and the net lock, as in C.
unsafe impl Sync for PfrKentry {}

/// `struct pfr_kentry_route`: an entry with an interface (route-to tables).
#[repr(C)]
pub struct PfrKentryRoute {
    /// The common part.
    pub ke: PfrKentry,
    /// `kif`.
    pub kif: Cell<Option<&'static PfiKif>>,
    /// `ifname`.
    pub ifname: Cell<[u8; IFNAMSIZ]>,
}

/// `struct pfr_kentry_cost`: an entry with an interface and a weight.
#[repr(C)]
pub struct PfrKentryCost {
    /// The common part.
    pub ke: PfrKentry,
    /// `kif`.
    pub kif: Cell<Option<&'static PfiKif>>,
    /// `ifname`. Above overlaps with `pfr_kentry_route`.
    pub ifname: Cell<[u8; IFNAMSIZ]>,
    /// `weight`.
    pub weight: Cell<u16>,
}

/// `struct pfr_kentry_all`: the largest of the entry types (the pool's item size).
#[repr(C)]
pub union PfrKentryAll {
    /// `_ke`.
    pub ke: core::mem::ManuallyDrop<PfrKentry>,
    /// `kr`.
    pub kr: core::mem::ManuallyDrop<PfrKentryRoute>,
    /// `kc`.
    pub kc: core::mem::ManuallyDrop<PfrKentryCost>,
}

crate::queue_adapter!(
    /// `SLIST_HEAD(pfr_kentryworkq, pfr_kentry)`.
    pub PfrKentryworkq: PfrKentry, pfrke_workq => SlistEntry<PfrKentry>
);
crate::queue_adapter!(
    /// The `pfrke_ioq` list (`SLIST_HEAD(, pfr_kentry)`).
    pub PfrKentryioq: PfrKentry, pfrke_ioq => SlistEntry<PfrKentry>
);

/// `struct pfr_ktable`: a table.
pub struct PfrKtable {
    /// `pfrkt_ts`: the table and its counters (`pfrkt_t`, `pfrkt_name`, ... are its members).
    pub pfrkt_ts: PfrTstats,
    /// `pfrkt_tree`.
    pub pfrkt_tree: RbEntry<PfrKtable>,
    /// `pfrkt_workq`.
    pub pfrkt_workq: SlistEntry<PfrKtable>,
    /// `pfrkt_ip4`.
    pub pfrkt_ip4: Cell<Option<&'static RadixNodeHead>>,
    /// `pfrkt_ip6`.
    pub pfrkt_ip6: Cell<Option<&'static RadixNodeHead>>,
    /// `pfrkt_shadow`.
    pub pfrkt_shadow: Cell<Option<&'static PfrKtable>>,
    /// `pfrkt_root`.
    pub pfrkt_root: Cell<Option<&'static PfrKtable>>,
    /// `pfrkt_rs`.
    pub pfrkt_rs: Cell<Option<&'static PfRuleset>>,
    /// `pfrkt_larg`.
    pub pfrkt_larg: Cell<i64>,
    /// `pfrkt_nflags`.
    pub pfrkt_nflags: Cell<i32>,
    /// `pfrkt_refcntcost`.
    pub pfrkt_refcntcost: Cell<u64>,
    /// `pfrkt_gcdweight`.
    pub pfrkt_gcdweight: Cell<u16>,
    /// `pfrkt_maxweight`.
    pub pfrkt_maxweight: Cell<u16>,
}

// SAFETY: changed under `pf_lock` and the net lock, as in C.
unsafe impl Sync for PfrKtable {}

impl PfrKtable {
    /// `pfrkt_t`.
    pub fn pfrkt_t(&self) -> &PfrTable {
        &self.pfrkt_ts.pfrts_t
    }

    /// `pfrkt_name`.
    pub fn pfrkt_name(&self) -> &[u8; PF_TABLE_NAME_SIZE] {
        &self.pfrkt_ts.pfrts_t.pfrt_name
    }

    /// `pfrkt_anchor`.
    pub fn pfrkt_anchor(&self) -> &[u8; PATH_MAX] {
        &self.pfrkt_ts.pfrts_t.pfrt_anchor
    }

    /// `pfrkt_flags`.
    pub fn pfrkt_flags(&self) -> &Cell<u32> {
        &self.pfrkt_ts.pfrts_t.pfrt_flags
    }

    /// `pfrkt_cnt`.
    pub fn pfrkt_cnt(&self) -> &Cell<i32> {
        &self.pfrkt_ts.pfrts_cnt
    }

    /// `pfrkt_refcnt`.
    pub fn pfrkt_refcnt(&self) -> &[Cell<i32>; PFR_REFCNT_MAX] {
        &self.pfrkt_ts.pfrts_refcnt
    }

    /// `pfrkt_packets`.
    pub fn pfrkt_packets(&self) -> &[[Cell<u64>; PFR_OP_TABLE_MAX]; PFR_DIR_MAX] {
        &self.pfrkt_ts.pfrts_packets
    }

    /// `pfrkt_bytes`.
    pub fn pfrkt_bytes(&self) -> &[[Cell<u64>; PFR_OP_TABLE_MAX]; PFR_DIR_MAX] {
        &self.pfrkt_ts.pfrts_bytes
    }

    /// `pfrkt_match`.
    pub fn pfrkt_match(&self) -> &Cell<u64> {
        &self.pfrkt_ts.pfrts_match
    }

    /// `pfrkt_nomatch`.
    pub fn pfrkt_nomatch(&self) -> &Cell<u64> {
        &self.pfrkt_ts.pfrts_nomatch
    }

    /// `pfrkt_tzero`.
    pub fn pfrkt_tzero(&self) -> &Cell<Time> {
        &self.pfrkt_ts.pfrts_tzero
    }
}

crate::queue_adapter!(
    /// `SLIST_HEAD(pfr_ktableworkq, pfr_ktable)`.
    pub PfrKtableworkq: PfrKtable, pfrkt_workq => SlistEntry<PfrKtable>
);

/// `struct pfi_kif`: pf's view of an interface or an interface group.
#[repr(C)]
pub struct PfiKif {
    /// `pfik_name`.
    pub pfik_name: [u8; IFNAMSIZ],
    /// `pfik_tree`: in `pfi_ifs`.
    pub pfik_tree: RbEntry<PfiKif>,
    /// `pfik_packets[2][2][2]`.
    pub pfik_packets: [[[Cell<u64>; 2]; 2]; 2],
    /// `pfik_bytes[2][2][2]`.
    pub pfik_bytes: [[[Cell<u64>; 2]; 2]; 2],
    /// `pfik_tzero`.
    pub pfik_tzero: Cell<Time>,
    /// `pfik_flags`.
    pub pfik_flags: Cell<i32>,
    /// `pfik_flags_new`.
    pub pfik_flags_new: Cell<i32>,
    /// `pfik_ah_cookie`.
    pub pfik_ah_cookie: Cell<*mut c_void>,
    /// `pfik_ifp`.
    pub pfik_ifp: Cell<*const Ifnet>,
    /// `pfik_group`.
    pub pfik_group: Cell<*const IfgGroup>,
    /// `pfik_states`.
    pub pfik_states: Cell<i32>,
    /// `pfik_rules`.
    pub pfik_rules: Cell<i32>,
    /// `pfik_routes`.
    pub pfik_routes: Cell<i32>,
    /// `pfik_srcnodes`.
    pub pfik_srcnodes: Cell<i32>,
    /// `pfik_flagrefs`.
    pub pfik_flagrefs: Cell<i32>,
    /// A hole the C compiler leaves.
    pub _pad: [u8; 4],
    /// `pfik_dynaddrs`.
    pub pfik_dynaddrs: TailqHead<PfiDynaddrList>,
}

// SAFETY: changed under the net lock and `pf_lock`, as in C.
unsafe impl Sync for PfiKif {}

// SAFETY: `#[repr(C)]`: bytes, `Cell`s of integers and raw words, the hole named `_pad`; the
// tree entry is three raw pointers and a colour word with its padding (as for `PfSrcNode`,
// whose 4 trailing bytes are covered by the zeroing and never read as a value), the queue
// head two raw words. `DIOCIGETIFACES` copies kifs out byte for byte (`pfi_get_ifaces`).
unsafe impl PfAbi for PfiKif {}

crate::tree_adapter!(
    /// `RB_HEAD(pfi_ifhead, pfi_kif)`: the kifs by name (`pfi_ifs`).
    pub PfiIfhead: PfiKif, pfik_tree => RbEntry<PfiKif>, crate::net::pf_if::pfi_if_compare
);

impl PfiKif {
    /// `pfik_ifp` of a kernel kif.
    pub fn pfik_ifp(&'static self) -> Option<&'static Ifnet> {
        // SAFETY: set by `pfi_attach_ifnet` to an attached interface and cleared by
        // `pfi_detach_ifnet` before the interface goes away.
        unsafe { self.pfik_ifp.get().as_ref() }
    }

    /// `pfik_ifp = ifp`.
    pub fn set_pfik_ifp(&self, ifp: Option<&'static Ifnet>) {
        self.pfik_ifp.set(ifp.map_or(ptr::null(), ptr::from_ref));
    }

    /// `pfik_group` of a kernel kif.
    pub fn pfik_group(&'static self) -> Option<&'static IfgGroup> {
        // SAFETY: set by `pfi_attach_ifgroup`, cleared by `pfi_detach_ifgroup`.
        unsafe { self.pfik_group.get().as_ref() }
    }

    /// `pfik_group = g`.
    pub fn set_pfik_group(&self, g: Option<&'static IfgGroup>) {
        self.pfik_group.set(g.map_or(ptr::null(), ptr::from_ref));
    }
}

/// `enum pfi_kif_refs`.
pub type PfiKifRefs = i32;
/// `PFI_KIF_REF_NONE`.
pub const PFI_KIF_REF_NONE: PfiKifRefs = 0;
/// `PFI_KIF_REF_STATE`.
pub const PFI_KIF_REF_STATE: PfiKifRefs = 1;
/// `PFI_KIF_REF_RULE`.
pub const PFI_KIF_REF_RULE: PfiKifRefs = 2;
/// `PFI_KIF_REF_ROUTE`.
pub const PFI_KIF_REF_ROUTE: PfiKifRefs = 3;
/// `PFI_KIF_REF_SRCNODE`.
pub const PFI_KIF_REF_SRCNODE: PfiKifRefs = 4;
/// `PFI_KIF_REF_FLAG`.
pub const PFI_KIF_REF_FLAG: PfiKifRefs = 5;

/// `PFI_IFLAG_SKIP`: skip filtering on interface.
pub const PFI_IFLAG_SKIP: i32 = 0x0100;
/// `PFI_IFLAG_ANY`: match any non-loopback interface.
pub const PFI_IFLAG_ANY: i32 = 0x0200;

/// `PF_DPORT_RANGE`: dest port uses range.
pub const PF_DPORT_RANGE: u8 = 0x01;
/// `PF_RPORT_RANGE`: RDR'ed port uses range.
pub const PF_RPORT_RANGE: u8 = 0x02;

/// `PFRES_MATCH`: explicit match of a rule.
pub const PFRES_MATCH: u16 = 0;
/// `PFRES_BADOFF`: bad offset for pull_hdr.
pub const PFRES_BADOFF: u16 = 1;
/// `PFRES_FRAG`: dropping following fragment.
pub const PFRES_FRAG: u16 = 2;
/// `PFRES_SHORT`: dropping short packet.
pub const PFRES_SHORT: u16 = 3;
/// `PFRES_NORM`: dropping by normalizer.
pub const PFRES_NORM: u16 = 4;
/// `PFRES_MEMORY`: dropped due to lacking mem.
pub const PFRES_MEMORY: u16 = 5;
/// `PFRES_TS`: bad TCP timestamp (RFC1323).
pub const PFRES_TS: u16 = 6;
/// `PFRES_CONGEST`: congestion.
pub const PFRES_CONGEST: u16 = 7;
/// `PFRES_IPOPTIONS`: IP option.
pub const PFRES_IPOPTIONS: u16 = 8;
/// `PFRES_PROTCKSUM`: protocol checksum invalid.
pub const PFRES_PROTCKSUM: u16 = 9;
/// `PFRES_BADSTATE`: state mismatch.
pub const PFRES_BADSTATE: u16 = 10;
/// `PFRES_STATEINS`: state insertion failure.
pub const PFRES_STATEINS: u16 = 11;
/// `PFRES_MAXSTATES`: state limit.
pub const PFRES_MAXSTATES: u16 = 12;
/// `PFRES_SRCLIMIT`: source node/conn limit.
pub const PFRES_SRCLIMIT: u16 = 13;
/// `PFRES_SYNPROXY`: SYN proxy.
pub const PFRES_SYNPROXY: u16 = 14;
/// `PFRES_TRANSLATE`: no translation address available.
pub const PFRES_TRANSLATE: u16 = 15;
/// `PFRES_NOROUTE`: no route found for PBR action.
pub const PFRES_NOROUTE: u16 = 16;
/// `PFRES_MAX`: total + 1.
pub const PFRES_MAX: usize = 17;

/// `PFRES_NAMES`.
pub const PFRES_NAMES: [&str; PFRES_MAX] = [
    "match",
    "bad-offset",
    "fragment",
    "short",
    "normalize",
    "memory",
    "bad-timestamp",
    "congestion",
    "ip-option",
    "proto-cksum",
    "state-mismatch",
    "state-insert",
    "state-limit",
    "src-limit",
    "synproxy",
    "translate",
    "no-route",
];

/// `LCNT_STATES`: states.
pub const LCNT_STATES: usize = 0;
/// `LCNT_SRCSTATES`: max-src-states.
pub const LCNT_SRCSTATES: usize = 1;
/// `LCNT_SRCNODES`: max-src-nodes.
pub const LCNT_SRCNODES: usize = 2;
/// `LCNT_SRCCONN`: max-src-conn.
pub const LCNT_SRCCONN: usize = 3;
/// `LCNT_SRCCONNRATE`: max-src-conn-rate.
pub const LCNT_SRCCONNRATE: usize = 4;
/// `LCNT_OVERLOAD_TABLE`: entry added to overload table.
pub const LCNT_OVERLOAD_TABLE: usize = 5;
/// `LCNT_OVERLOAD_FLUSH`: state entries flushed.
pub const LCNT_OVERLOAD_FLUSH: usize = 6;
/// `LCNT_SYNFLOODS`: synfloods detected.
pub const LCNT_SYNFLOODS: usize = 7;
/// `LCNT_SYNCOOKIES_SENT`: syncookies sent.
pub const LCNT_SYNCOOKIES_SENT: usize = 8;
/// `LCNT_SYNCOOKIES_VALID`: syncookies validated.
pub const LCNT_SYNCOOKIES_VALID: usize = 9;
/// `LCNT_MAX`: total + 1.
pub const LCNT_MAX: usize = 10;

/// `PFUDPS_NO_TRAFFIC`.
pub const PFUDPS_NO_TRAFFIC: u8 = 0;
/// `PFUDPS_SINGLE`.
pub const PFUDPS_SINGLE: u8 = 1;
/// `PFUDPS_MULTIPLE`.
pub const PFUDPS_MULTIPLE: u8 = 2;
/// `PFUDPS_NSTATES`: number of state levels.
pub const PFUDPS_NSTATES: u8 = 3;
/// `PFOTHERS_NO_TRAFFIC`.
pub const PFOTHERS_NO_TRAFFIC: u8 = 0;
/// `PFOTHERS_SINGLE`.
pub const PFOTHERS_SINGLE: u8 = 1;
/// `PFOTHERS_MULTIPLE`.
pub const PFOTHERS_MULTIPLE: u8 = 2;
/// `PFOTHERS_NSTATES`: number of state levels.
pub const PFOTHERS_NSTATES: u8 = 3;

/// `FCNT_STATE_SEARCH`.
pub const FCNT_STATE_SEARCH: usize = 0;
/// `FCNT_STATE_INSERT`.
pub const FCNT_STATE_INSERT: usize = 1;
/// `FCNT_STATE_REMOVALS`.
pub const FCNT_STATE_REMOVALS: usize = 2;
/// `FCNT_MAX`.
pub const FCNT_MAX: usize = 3;
/// `SCNT_SRC_NODE_SEARCH`.
pub const SCNT_SRC_NODE_SEARCH: usize = 0;
/// `SCNT_SRC_NODE_INSERT`.
pub const SCNT_SRC_NODE_INSERT: usize = 1;
/// `SCNT_SRC_NODE_REMOVALS`.
pub const SCNT_SRC_NODE_REMOVALS: usize = 2;
/// `SCNT_MAX`.
pub const SCNT_MAX: usize = 3;
/// `NCNT_FRAG_SEARCH`.
pub const NCNT_FRAG_SEARCH: usize = 0;
/// `NCNT_FRAG_INSERT`.
pub const NCNT_FRAG_INSERT: usize = 1;
/// `NCNT_FRAG_REMOVALS`.
pub const NCNT_FRAG_REMOVALS: usize = 2;
/// `NCNT_MAX`.
pub const NCNT_MAX: usize = 3;

/// `REASON_SET(a, x)`: records the reason `x` and counts it.
pub fn reason_set(a: &mut u16, x: u16) {
    *a = x;
    if usize::from(x) < PFRES_MAX {
        let c = &crate::net::pf::PF_STATUS.counters[usize::from(x)];
        c.set(c.get().wrapping_add(1));
    }
}

/// `struct pf_status`: pf's global status and counters, `DIOCGETSTATUS`'s result.
#[repr(C)]
pub struct PfStatus {
    /// `counters[PFRES_MAX]`.
    pub counters: [Cell<u64>; PFRES_MAX],
    /// `lcounters[LCNT_MAX]`: limit counters.
    pub lcounters: [Cell<u64>; LCNT_MAX],
    /// `fcounters[FCNT_MAX]`.
    pub fcounters: [Cell<u64>; FCNT_MAX],
    /// `scounters[SCNT_MAX]`.
    pub scounters: [Cell<u64>; SCNT_MAX],
    /// `ncounters[NCNT_MAX]`.
    pub ncounters: [Cell<u64>; NCNT_MAX],
    /// `pcounters[2][2][3]`.
    pub pcounters: [[[Cell<u64>; 3]; 2]; 2],
    /// `bcounters[2][2]`.
    pub bcounters: [[Cell<u64>; 2]; 2],
    /// `stateid`.
    pub stateid: Cell<u64>,
    /// `syncookies_inflight[2]`: unACKed SYNcookies.
    pub syncookies_inflight: [Cell<u64>; 2],
    /// `since`.
    pub since: Cell<Time>,
    /// `running`.
    pub running: Cell<u32>,
    /// `states`.
    pub states: Cell<u32>,
    /// `states_halfopen`.
    pub states_halfopen: Cell<u32>,
    /// `src_nodes`.
    pub src_nodes: Cell<u32>,
    /// `fragments`.
    pub fragments: Cell<u32>,
    /// `debug`.
    pub debug: Cell<u32>,
    /// `hostid`.
    pub hostid: Cell<u32>,
    /// `reass`: reassembly.
    pub reass: Cell<u32>,
    /// `syncookies_active`.
    pub syncookies_active: Cell<u8>,
    /// `syncookies_mode`: never/always/adaptive.
    pub syncookies_mode: Cell<u8>,
    /// `pad[2]`.
    pub pad: [Cell<u8>; 2],
    /// `ifname`.
    pub ifname: Cell<[u8; IFNAMSIZ]>,
    /// `pf_chksum`.
    pub pf_chksum: Cell<[u8; PF_MD5_DIGEST_LENGTH]>,
    /// A hole the C compiler leaves.
    pub _pad: [u8; 4],
}

// SAFETY: changed under the net lock, `pf_lock` and `pf_frag_mtx`, as in C.
unsafe impl Sync for PfStatus {}

// SAFETY: `#[repr(C)]` `Cell`s of integers and byte arrays, the tail hole named `_pad`.
unsafe impl PfAbi for PfStatus {}

impl PfStatus {
    /// An all-zero status.
    pub const fn zeroed() -> Self {
        // SAFETY: `PfAbi`: every bit pattern is valid.
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
}

/// `PF_REASS_ENABLED`.
pub const PF_REASS_ENABLED: u32 = 0x01;
/// `PF_REASS_NODF`.
pub const PF_REASS_NODF: u32 = 0x02;

/// `PF_SYNCOOKIES_NEVER`.
pub const PF_SYNCOOKIES_NEVER: u8 = 0;
/// `PF_SYNCOOKIES_ALWAYS`.
pub const PF_SYNCOOKIES_ALWAYS: u8 = 1;
/// `PF_SYNCOOKIES_ADAPTIVE`.
pub const PF_SYNCOOKIES_ADAPTIVE: u8 = 2;
/// `PF_SYNCOOKIES_MODE_MAX`.
pub const PF_SYNCOOKIES_MODE_MAX: u8 = PF_SYNCOOKIES_ADAPTIVE;
/// `PF_SYNCOOKIES_HIWATPCT`.
pub const PF_SYNCOOKIES_HIWATPCT: u32 = 25;
/// `PF_SYNCOOKIES_LOWATPCT`.
pub const PF_SYNCOOKIES_LOWATPCT: u32 = PF_SYNCOOKIES_HIWATPCT / 2;

/// `PF_PRIO_ZERO`: match "prio 0" packets.
pub const PF_PRIO_ZERO: u8 = 0xff;

/// `struct pf_queue_bwspec`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfQueueBwspec {
    /// `absolute`.
    pub absolute: u64,
    /// `percent`.
    pub percent: u32,
    /// A hole the C compiler leaves.
    pub _pad: [u8; 4],
}

/// `struct pf_queue_scspec`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfQueueScspec {
    /// `m1`.
    pub m1: PfQueueBwspec,
    /// `m2`.
    pub m2: PfQueueBwspec,
    /// `d`.
    pub d: u32,
    /// A hole the C compiler leaves.
    pub _pad: [u8; 4],
}

/// `struct pf_queue_fqspec`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfQueueFqspec {
    /// `flows`.
    pub flows: u32,
    /// `quantum`.
    pub quantum: u32,
    /// `target`.
    pub target: u32,
    /// `interval`.
    pub interval: u32,
}

/// `struct pf_queuespec`: a queue of a queueing ruleset.
#[repr(C)]
pub struct PfQueuespec {
    /// `entries`.
    pub entries: TailqEntry<PfQueuespec>,
    /// `qname`.
    pub qname: [u8; PF_QNAME_SIZE],
    /// `parent`.
    pub parent: [u8; PF_QNAME_SIZE],
    /// `ifname`.
    pub ifname: [u8; IFNAMSIZ],
    /// `realtime`.
    pub realtime: PfQueueScspec,
    /// `linkshare`.
    pub linkshare: PfQueueScspec,
    /// `upperlimit`.
    pub upperlimit: PfQueueScspec,
    /// `flowqueue`.
    pub flowqueue: PfQueueFqspec,
    /// `kif`.
    pub kif: Cell<*const PfiKif>,
    /// `flags`: `PFQS_*`.
    pub flags: u32,
    /// `qlimit`.
    pub qlimit: u32,
    /// `qid`.
    pub qid: u32,
    /// `parent_qid`.
    pub parent_qid: u32,
}

// SAFETY: a kernel queue spec is changed only under `pf_lock` (its kif), as in C.
unsafe impl Sync for PfQueuespec {}

// SAFETY: `#[repr(C)]` integers, byte arrays, raw words and plain-data structures whose holes
// are named; any bit pattern is valid.
unsafe impl PfAbi for PfQueuespec {}

impl PfQueuespec {
    /// An all-zero spec.
    pub const fn zeroed() -> Self {
        // SAFETY: `PfAbi`: every bit pattern is valid.
        unsafe { MaybeUninit::zeroed().assume_init() }
    }

    /// `kif` of a kernel queue.
    pub fn kif(&'static self) -> Option<&'static PfiKif> {
        // SAFETY: set by `DIOCADDQUEUE` to a referenced kif, unreferenced by
        // `pf_remove_queues` with the spec.
        unsafe { self.kif.get().as_ref() }
    }

    /// `kif = k`.
    pub fn set_kif(&self, k: Option<&'static PfiKif>) {
        self.kif.set(k.map_or(ptr::null(), ptr::from_ref));
    }
}

/// `PFQS_FLOWQUEUE`.
pub const PFQS_FLOWQUEUE: u32 = 0x0001;
/// `PFQS_ROOTCLASS`.
pub const PFQS_ROOTCLASS: u32 = 0x0002;
/// `PFQS_DEFAULT`: maps to `HFSC_DEFAULTCLASS`.
pub const PFQS_DEFAULT: u32 = 0x1000;

/// `struct priq_opts`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PriqOpts {
    /// `flags`.
    pub flags: i32,
}

/// `struct hfsc_opts`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct HfscOpts {
    /// `rtsc_m1`: slope of the 1st segment in bps (real-time service curve).
    pub rtsc_m1: u32,
    /// `rtsc_d`: the x-projection of m1 in msec.
    pub rtsc_d: u32,
    /// `rtsc_m2`: slope of the 2nd segment in bps.
    pub rtsc_m2: u32,
    /// `lssc_m1` (link-sharing service curve).
    pub lssc_m1: u32,
    /// `lssc_d`.
    pub lssc_d: u32,
    /// `lssc_m2`.
    pub lssc_m2: u32,
    /// `ulsc_m1` (upper-limit service curve).
    pub ulsc_m1: u32,
    /// `ulsc_d`.
    pub ulsc_d: u32,
    /// `ulsc_m2`.
    pub ulsc_m2: u32,
    /// `flags`.
    pub flags: i32,
}

/// `struct pfq_ops`: a queueing discipline as pf configures it (HFSC, FQ-CoDel). The
/// discipline's state is the opaque pointer its `pfq_alloc` returned.
pub struct PfqOps {
    /// `pfq_alloc`.
    pub pfq_alloc: fn(&'static Ifnet) -> *mut c_void,
    /// `pfq_addqueue`.
    pub pfq_addqueue: fn(*mut c_void, &'static PfQueuespec) -> Result<(), Errno>,
    /// `pfq_free`.
    pub pfq_free: fn(*mut c_void),
    /// `pfq_qstats`: copies the statistics to `ubuf`, a user buffer of `*nbytes` bytes.
    pub pfq_qstats: fn(&'static PfQueuespec, usize, &mut i32) -> Result<(), Errno>,
    /// `pfq_qlength`.
    pub pfq_qlength: fn(*mut c_void) -> u32,
    /// `pfq_enqueue`: the packet dropped, if any.
    pub pfq_enqueue: fn(*mut c_void, &'static Mbuf) -> Option<&'static Mbuf>,
    /// `pfq_deq_begin`.
    pub pfq_deq_begin:
        fn(*mut c_void, &mut *mut c_void, &crate::sys::mbuf::MbufList) -> Option<&'static Mbuf>,
    /// `pfq_deq_commit`.
    pub pfq_deq_commit: fn(*mut c_void, &'static Mbuf, *mut c_void),
    /// `pfq_purge`.
    pub pfq_purge: fn(*mut c_void, &crate::sys::mbuf::MbufList),
}

/// `struct pf_tagname`: a tag or queue name and its number.
pub struct PfTagname {
    /// `entries`.
    pub entries: TailqEntry<PfTagname>,
    /// `name`.
    pub name: [u8; PF_TAG_NAME_SIZE],
    /// `tag`.
    pub tag: Cell<u16>,
    /// `ref`.
    pub ref_: Cell<i32>,
}

/// `struct pf_divert`: the divert-to address of a diverted packet (an mbuf tag).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfDivert {
    /// `addr`.
    pub addr: PfAddr,
    /// `port`.
    pub port: u16,
    /// `rdomain`.
    pub rdomain: u16,
    /// `type`: `PF_DIVERT_*`.
    pub type_: u8,
}

/// `PF_DIVERT_NONE`.
pub const PF_DIVERT_NONE: u8 = 0;
/// `PF_DIVERT_TO`.
pub const PF_DIVERT_TO: u8 = 1;
/// `PF_DIVERT_REPLY`.
pub const PF_DIVERT_REPLY: u8 = 2;
/// `PF_DIVERT_PACKET`.
pub const PF_DIVERT_PACKET: u8 = 3;

/// `struct pf_pktdelay`: a packet held back by `delay`.
pub struct PfPktdelay {
    /// `to`.
    pub to: Timeout,
    /// `m`.
    pub m: Cell<Option<&'static Mbuf>>,
    /// `ifidx`.
    pub ifidx: Cell<u32>,
}

/// `PFFRAG_FRENT_HIWAT`: number of fragment entries (entries reference mbuf clusters, so the
/// default is based on that).
pub const fn pffrag_frent_hiwat() -> u32 {
    crate::sys::param::NMBCLUSTERS as u32 / 16
}

/// `PFFRAG_FRAG_HIWAT`: number of packets in reassembly.
pub const fn pffrag_frag_hiwat() -> u32 {
    crate::sys::param::NMBCLUSTERS as u32 / 32
}

/// `PFR_KTABLE_HIWAT`: number of tables.
pub const PFR_KTABLE_HIWAT: u32 = 1000;
/// `PFR_KENTRY_HIWAT`: number of table entries.
pub const PFR_KENTRY_HIWAT: u32 = 200_000;
/// `PFR_KENTRY_HIWAT_SMALL`: number of entries for tiny hosts.
pub const PFR_KENTRY_HIWAT_SMALL: u32 = 100_000;

/*
 * ioctl parameter structures
 */

/// `struct pfioc_rule`.
#[repr(C)]
pub struct PfiocRule {
    /// `action`: `PF_CHANGE_*`, `PF_GET_*`.
    pub action: u32,
    /// `ticket`.
    pub ticket: u32,
    /// `nr`.
    pub nr: u32,
    /// `anchor`.
    pub anchor: [u8; PATH_MAX],
    /// `anchor_call`.
    pub anchor_call: [u8; PATH_MAX],
    /// A hole the C compiler leaves.
    pub _pad: [u8; 4],
    /// `rule`.
    pub rule: PfRule,
}

// SAFETY: `#[repr(C)]` integers, byte arrays and a `PfAbi` rule, the hole named `_pad`.
unsafe impl PfAbi for PfiocRule {}

/// `struct pfioc_natlook`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfiocNatlook {
    /// `saddr`.
    pub saddr: PfAddr,
    /// `daddr`.
    pub daddr: PfAddr,
    /// `rsaddr`.
    pub rsaddr: PfAddr,
    /// `rdaddr`.
    pub rdaddr: PfAddr,
    /// `rdomain`.
    pub rdomain: u16,
    /// `rrdomain`.
    pub rrdomain: u16,
    /// `sport`.
    pub sport: u16,
    /// `dport`.
    pub dport: u16,
    /// `rsport`.
    pub rsport: u16,
    /// `rdport`.
    pub rdport: u16,
    /// `af`.
    pub af: SaFamily,
    /// `proto`.
    pub proto: u8,
    /// `direction`.
    pub direction: u8,
    /// A hole the C compiler leaves.
    pub _pad: u8,
}

// SAFETY: integers, the hole named `_pad`.
unsafe impl AbiPod for PfiocNatlook {}

/// `struct pfioc_state`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PfiocState {
    /// `state`.
    pub state: PfsyncState,
}

// SAFETY: a packed `AbiPod`.
unsafe impl AbiPod for PfiocState {}

/// `struct pfioc_src_node_kill`.
#[repr(C)]
pub struct PfiocSrcNodeKill {
    /// `psnk_af`.
    pub psnk_af: SaFamily,
    /// A hole the C compiler leaves.
    pub _pad0: [u8; 7],
    /// `psnk_src`.
    pub psnk_src: PfRuleAddr,
    /// `psnk_dst`.
    pub psnk_dst: PfRuleAddr,
    /// `psnk_killed`.
    pub psnk_killed: u32,
    /// A hole the C compiler leaves.
    pub _pad1: [u8; 4],
}

// SAFETY: `#[repr(C)]` integers and `PfAbi` operands, holes named.
unsafe impl PfAbi for PfiocSrcNodeKill {}

/// `struct pfioc_state_kill`.
#[repr(C)]
pub struct PfiocStateKill {
    /// `psk_pfcmp`.
    pub psk_pfcmp: PfStateCmp,
    /// `psk_af`.
    pub psk_af: SaFamily,
    /// A hole the C compiler leaves.
    pub _pad0: [u8; 3],
    /// `psk_proto`.
    pub psk_proto: i32,
    /// `psk_src`.
    pub psk_src: PfRuleAddr,
    /// `psk_dst`.
    pub psk_dst: PfRuleAddr,
    /// `psk_ifname`.
    pub psk_ifname: [u8; IFNAMSIZ],
    /// `psk_label`.
    pub psk_label: [u8; PF_RULE_LABEL_SIZE],
    /// `psk_killed`.
    pub psk_killed: u32,
    /// `psk_rdomain`.
    pub psk_rdomain: u16,
    /// A hole the C compiler leaves.
    pub _pad1: [u8; 2],
}

// SAFETY: `#[repr(C)]` integers, byte arrays and `PfAbi` operands, holes named.
unsafe impl PfAbi for PfiocStateKill {}

/// `struct pfioc_states`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfiocStates {
    /// `ps_len`.
    pub ps_len: usize,
    /// `ps_buf` / `ps_states`: a user address.
    pub ps_buf: usize,
}

// SAFETY: two words.
unsafe impl AbiPod for PfiocStates {}

/// `struct pfioc_src_nodes`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfiocSrcNodes {
    /// `psn_len`.
    pub psn_len: usize,
    /// `psn_buf` / `psn_src_nodes`: a user address.
    pub psn_buf: usize,
}

// SAFETY: two words.
unsafe impl AbiPod for PfiocSrcNodes {}

/// `struct pfioc_tm`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfiocTm {
    /// `timeout`.
    pub timeout: i32,
    /// `seconds`.
    pub seconds: i32,
}

// SAFETY: two ints.
unsafe impl AbiPod for PfiocTm {}

/// `struct pfioc_limit`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfiocLimit {
    /// `index`.
    pub index: i32,
    /// `limit`.
    pub limit: u32,
}

// SAFETY: two ints.
unsafe impl AbiPod for PfiocLimit {}

/// `struct pfioc_ruleset`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PfiocRuleset {
    /// `nr`.
    pub nr: u32,
    /// `path`.
    pub path: [u8; PATH_MAX],
    /// `name`.
    pub name: [u8; PF_ANCHOR_NAME_SIZE],
}

// SAFETY: an int and byte arrays, no hole (1092 bytes, alignment 4).
unsafe impl AbiPod for PfiocRuleset {}

/// `struct pfioc_trans`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfiocTrans {
    /// `size`: number of elements.
    pub size: i32,
    /// `esize`: size of each element in bytes.
    pub esize: i32,
    /// `array`: a user address of `size` `struct pfioc_trans_e`.
    pub array: usize,
}

// SAFETY: two ints and a word.
unsafe impl AbiPod for PfiocTrans {}

/// `struct pfioc_trans_e`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PfiocTransE {
    /// `type`: `PF_TRANS_*`.
    pub type_: i32,
    /// `anchor`.
    pub anchor: [u8; PATH_MAX],
    /// `ticket`.
    pub ticket: u32,
}

// SAFETY: ints and a byte array, no hole.
unsafe impl AbiPod for PfiocTransE {}

/// `struct pfioc_queue`.
#[repr(C)]
pub struct PfiocQueue {
    /// `ticket`.
    pub ticket: u32,
    /// `nr`.
    pub nr: u32,
    /// `queue`.
    pub queue: PfQueuespec,
}

// SAFETY: ints and a `PfAbi` spec, no hole.
unsafe impl PfAbi for PfiocQueue {}

/// `struct pfioc_qstats`.
#[repr(C)]
pub struct PfiocQstats {
    /// `ticket`.
    pub ticket: u32,
    /// `nr`.
    pub nr: u32,
    /// `queue`.
    pub queue: PfQueuespec,
    /// `buf`: a user address.
    pub buf: usize,
    /// `nbytes`.
    pub nbytes: i32,
    /// A hole the C compiler leaves.
    pub _pad: [u8; 4],
}

// SAFETY: ints, a word and a `PfAbi` spec, the hole named.
unsafe impl PfAbi for PfiocQstats {}

/// `PFR_FLAG_DUMMY`.
pub const PFR_FLAG_DUMMY: i32 = 0x0000_0002;
/// `PFR_FLAG_FEEDBACK`.
pub const PFR_FLAG_FEEDBACK: i32 = 0x0000_0004;
/// `PFR_FLAG_CLSTATS`.
pub const PFR_FLAG_CLSTATS: i32 = 0x0000_0008;
/// `PFR_FLAG_ADDRSTOO`.
pub const PFR_FLAG_ADDRSTOO: i32 = 0x0000_0010;
/// `PFR_FLAG_REPLACE`.
pub const PFR_FLAG_REPLACE: i32 = 0x0000_0020;
/// `PFR_FLAG_ALLRSETS`.
pub const PFR_FLAG_ALLRSETS: i32 = 0x0000_0040;
/// `PFR_FLAG_ALLMASK`.
pub const PFR_FLAG_ALLMASK: i32 = 0x0000_007F;
/// `PFR_FLAG_USERIOCTL`.
pub const PFR_FLAG_USERIOCTL: i32 = 0x1000_0000;

/// `struct pfioc_table`.
#[repr(C)]
pub struct PfiocTable {
    /// `pfrio_table`.
    pub pfrio_table: PfrTable,
    /// `pfrio_buffer`: a user address.
    pub pfrio_buffer: usize,
    /// `pfrio_esize`.
    pub pfrio_esize: i32,
    /// `pfrio_size`.
    pub pfrio_size: i32,
    /// `pfrio_size2` (`pfrio_naddr`, `pfrio_setflag`).
    pub pfrio_size2: i32,
    /// `pfrio_nadd` (`pfrio_exists`, `pfrio_nzero`, `pfrio_nmatch`, `pfrio_clrflag`).
    pub pfrio_nadd: i32,
    /// `pfrio_ndel`.
    pub pfrio_ndel: i32,
    /// `pfrio_nchange`.
    pub pfrio_nchange: i32,
    /// `pfrio_flags`.
    pub pfrio_flags: i32,
    /// `pfrio_ticket`.
    pub pfrio_ticket: u32,
}

// SAFETY: a `PfAbi` table, a word and ints, no hole.
unsafe impl PfAbi for PfiocTable {}

/// `struct pfioc_iface`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfiocIface {
    /// `pfiio_name`.
    pub pfiio_name: [u8; IFNAMSIZ],
    /// `pfiio_buffer`: a user address.
    pub pfiio_buffer: usize,
    /// `pfiio_esize`.
    pub pfiio_esize: i32,
    /// `pfiio_size`.
    pub pfiio_size: i32,
    /// `pfiio_nzero`.
    pub pfiio_nzero: i32,
    /// `pfiio_flags`.
    pub pfiio_flags: i32,
}

// SAFETY: bytes, a word and ints, no hole.
unsafe impl AbiPod for PfiocIface {}

/// `struct pfioc_synflwats`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfiocSynflwats {
    /// `hiwat`.
    pub hiwat: u32,
    /// `lowat`.
    pub lowat: u32,
}

// SAFETY: two ints.
unsafe impl AbiPod for PfiocSynflwats {}

/// `PF_STATELIM_NAME_LEN`: kstat istr.
pub const PF_STATELIM_NAME_LEN: usize = 16;
/// `PF_STATELIM_DESCR_LEN`.
pub const PF_STATELIM_DESCR_LEN: usize = 64;
/// `PF_STATELIM_ID_NONE`.
pub const PF_STATELIM_ID_NONE: u32 = 0;
/// `PF_STATELIM_ID_MIN`.
pub const PF_STATELIM_ID_MIN: u32 = 1;
/// `PF_STATELIM_ID_MAX`: fits in pf_state's uint8_t.
pub const PF_STATELIM_ID_MAX: u32 = 255;
/// `PF_STATELIM_LIMIT_MIN`.
pub const PF_STATELIM_LIMIT_MIN: u32 = 1;
/// `PF_STATELIM_LIMIT_MAX`: pf is pretty scalable.
pub const PF_STATELIM_LIMIT_MAX: u32 = 1 << 24;

/// The `rate` member of the limiter ioctls.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfiocRate {
    /// `limit`.
    pub limit: u32,
    /// `seconds`.
    pub seconds: u32,
}

/// `struct pfioc_statelim`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PfiocStatelim {
    /// `ticket`.
    pub ticket: u32,
    /// `name`.
    pub name: [u8; PF_STATELIM_NAME_LEN],
    /// `id`.
    pub id: u32,
    /// `limit`: limit on the total number of states.
    pub limit: u32,
    /// `rate`: rate limit on the creation of states.
    pub rate: PfiocRate,
    /// `description`.
    pub description: [u8; PF_STATELIM_DESCR_LEN],
    /// `inuse`: gauge (kernel state for GET ioctls).
    pub inuse: u32,
    /// `admitted`: counter.
    pub admitted: u64,
    /// `hardlimited`: counter.
    pub hardlimited: u64,
    /// `ratelimited`: counter.
    pub ratelimited: u64,
}

// SAFETY: ints and byte arrays, no hole (128 bytes).
unsafe impl AbiPod for PfiocStatelim {}

/// `PF_SOURCELIM_NAME_LEN`: kstat istr.
pub const PF_SOURCELIM_NAME_LEN: usize = 16;
/// `PF_SOURCELIM_DESCR_LEN`.
pub const PF_SOURCELIM_DESCR_LEN: usize = 64;
/// `PF_SOURCELIM_ID_NONE`.
pub const PF_SOURCELIM_ID_NONE: u32 = 0;
/// `PF_SOURCELIM_ID_MIN`.
pub const PF_SOURCELIM_ID_MIN: u32 = 1;
/// `PF_SOURCELIM_ID_MAX`: fits in pf_state's uint8_t.
pub const PF_SOURCELIM_ID_MAX: u32 = 255;

/// `struct pfioc_sourcelim`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PfiocSourcelim {
    /// `ticket`.
    pub ticket: u32,
    /// `name`.
    pub name: [u8; PF_SOURCELIM_NAME_LEN],
    /// `id`.
    pub id: u32,
    /// `entries`: limit on the total number of address entries.
    pub entries: u32,
    /// `limit`: limit on the number of states per address entry.
    pub limit: u32,
    /// `rate`: rate limit on the creation of states by an address entry.
    pub rate: PfiocRate,
    /// `overload_tblname`: when the number of states on an entry exceeds hwm, add the address
    /// to this table; when it goes below lwm, remove it.
    pub overload_tblname: [u8; PF_TABLE_NAME_SIZE],
    /// `overload_hwm`.
    pub overload_hwm: u32,
    /// `overload_lwm`.
    pub overload_lwm: u32,
    /// `inet_prefix`: mask addresses before they're used for entries.
    pub inet_prefix: u32,
    /// `inet6_prefix`.
    pub inet6_prefix: u32,
    /// `description`.
    pub description: [u8; PF_SOURCELIM_DESCR_LEN],
    /// `nentries`: gauge.
    pub nentries: u32,
    /// `inuse`: gauge.
    pub inuse: u32,
    /// `addrallocs`: counter.
    pub addrallocs: u64,
    /// `addrnomem`: counter.
    pub addrnomem: u64,
    /// `admitted`: counter.
    pub admitted: u64,
    /// `addrlimited`: counter.
    pub addrlimited: u64,
    /// `hardlimited`: counter.
    pub hardlimited: u64,
    /// `ratelimited`: counter.
    pub ratelimited: u64,
}

// SAFETY: ints and byte arrays, no hole (208 bytes).
unsafe impl AbiPod for PfiocSourcelim {}

/// `struct pfioc_source_entry`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfiocSourceEntry {
    /// `af`.
    pub af: SaFamily,
    /// A hole the C compiler leaves.
    pub _pad0: [u8; 3],
    /// `rdomain`.
    pub rdomain: u32,
    /// `addr`.
    pub addr: PfAddr,
    /// `inuse`: gauge.
    pub inuse: u32,
    /// A hole the C compiler leaves.
    pub _pad1: [u8; 4],
    /// `admitted`: counter.
    pub admitted: u64,
    /// `hardlimited`: counter.
    pub hardlimited: u64,
    /// `ratelimited`: counter.
    pub ratelimited: u64,
}

// SAFETY: ints and bytes, holes named.
unsafe impl AbiPod for PfiocSourceEntry {}

/// `struct pfioc_source`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfiocSource {
    /// `name`.
    pub name: [u8; PF_SOURCELIM_NAME_LEN],
    /// `id`.
    pub id: u32,
    /// `inet_prefix`: copied from the parent source limiter.
    pub inet_prefix: u32,
    /// `inet6_prefix`.
    pub inet6_prefix: u32,
    /// `limit`.
    pub limit: u32,
    /// `entry_size`: `sizeof(struct pfioc_source_entry)`.
    pub entry_size: usize,
    /// `key`: a user address.
    pub key: usize,
    /// `entries`: a user address.
    pub entries: usize,
    /// `entrieslen`: bytes.
    pub entrieslen: usize,
}

// SAFETY: ints, bytes and words, no hole.
unsafe impl AbiPod for PfiocSource {}

/// `struct pfioc_source_kill`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfiocSourceKill {
    /// `name`.
    pub name: [u8; PF_SOURCELIM_NAME_LEN],
    /// `id`.
    pub id: u32,
    /// `rdomain`.
    pub rdomain: u32,
    /// `af`.
    pub af: SaFamily,
    /// A hole the C compiler leaves.
    pub _pad: [u8; 3],
    /// `addr`.
    pub addr: PfAddr,
    /// `rmstates`: kill the states too?
    pub rmstates: u32,
}

// SAFETY: ints and bytes, the hole named.
unsafe impl AbiPod for PfiocSourceKill {}

// SAFETY: `#[repr(C)]`, 16 bytes of `u8`.
unsafe impl AbiPod for PfAddr {}

/// `_IO`, `_IOW` and `_IOWR` of the `'D'` group.
const fn dio(n: u8) -> u64 {
    crate::sys::ioccom::_io(b'D', n)
}
const fn diowr<T>(n: u8) -> u64 {
    crate::sys::ioccom::_iowr::<T>(b'D', n)
}
const fn diow<T>(n: u8) -> u64 {
    crate::sys::ioccom::_iow::<T>(b'D', n)
}

/// `DIOCSTART`.
pub const DIOCSTART: u64 = dio(1);
/// `DIOCSTOP`.
pub const DIOCSTOP: u64 = dio(2);
/// `DIOCADDRULE`.
pub const DIOCADDRULE: u64 = diowr::<PfiocRule>(4);
/// `DIOCGETRULES`.
pub const DIOCGETRULES: u64 = diowr::<PfiocRule>(6);
/// `DIOCGETRULE`.
pub const DIOCGETRULE: u64 = diowr::<PfiocRule>(7);
/// `DIOCCLRSTATES`.
pub const DIOCCLRSTATES: u64 = diowr::<PfiocStateKill>(18);
/// `DIOCGETSTATE`.
pub const DIOCGETSTATE: u64 = diowr::<PfiocState>(19);
/// `DIOCSETSTATUSIF`.
pub const DIOCSETSTATUSIF: u64 = diowr::<PfiocIface>(20);
/// `DIOCGETSTATUS`.
pub const DIOCGETSTATUS: u64 = diowr::<PfStatus>(21);
/// `DIOCCLRSTATUS`.
pub const DIOCCLRSTATUS: u64 = diowr::<PfiocIface>(22);
/// `DIOCNATLOOK`.
pub const DIOCNATLOOK: u64 = diowr::<PfiocNatlook>(23);
/// `DIOCSETDEBUG`.
pub const DIOCSETDEBUG: u64 = diowr::<u32>(24);
/// `DIOCGETSTATES`.
pub const DIOCGETSTATES: u64 = diowr::<PfiocStates>(25);
/// `DIOCCHANGERULE`.
pub const DIOCCHANGERULE: u64 = diowr::<PfiocRule>(26);
/// `DIOCSETTIMEOUT`.
pub const DIOCSETTIMEOUT: u64 = diowr::<PfiocTm>(29);
/// `DIOCGETTIMEOUT`.
pub const DIOCGETTIMEOUT: u64 = diowr::<PfiocTm>(30);
/// `DIOCADDSTATE`.
pub const DIOCADDSTATE: u64 = diowr::<PfiocState>(37);
/// `DIOCGETLIMIT`.
pub const DIOCGETLIMIT: u64 = diowr::<PfiocLimit>(39);
/// `DIOCSETLIMIT`.
pub const DIOCSETLIMIT: u64 = diowr::<PfiocLimit>(40);
/// `DIOCKILLSTATES`.
pub const DIOCKILLSTATES: u64 = diowr::<PfiocStateKill>(41);
/// `DIOCGETRULESETS`.
pub const DIOCGETRULESETS: u64 = diowr::<PfiocRuleset>(58);
/// `DIOCGETRULESET`.
pub const DIOCGETRULESET: u64 = diowr::<PfiocRuleset>(59);
/// `DIOCRCLRTABLES`.
pub const DIOCRCLRTABLES: u64 = diowr::<PfiocTable>(60);
/// `DIOCRADDTABLES`.
pub const DIOCRADDTABLES: u64 = diowr::<PfiocTable>(61);
/// `DIOCRDELTABLES`.
pub const DIOCRDELTABLES: u64 = diowr::<PfiocTable>(62);
/// `DIOCRGETTABLES`.
pub const DIOCRGETTABLES: u64 = diowr::<PfiocTable>(63);
/// `DIOCRGETTSTATS`.
pub const DIOCRGETTSTATS: u64 = diowr::<PfiocTable>(64);
/// `DIOCRCLRTSTATS`.
pub const DIOCRCLRTSTATS: u64 = diowr::<PfiocTable>(65);
/// `DIOCRCLRADDRS`.
pub const DIOCRCLRADDRS: u64 = diowr::<PfiocTable>(66);
/// `DIOCRADDADDRS`.
pub const DIOCRADDADDRS: u64 = diowr::<PfiocTable>(67);
/// `DIOCRDELADDRS`.
pub const DIOCRDELADDRS: u64 = diowr::<PfiocTable>(68);
/// `DIOCRSETADDRS`.
pub const DIOCRSETADDRS: u64 = diowr::<PfiocTable>(69);
/// `DIOCRGETADDRS`.
pub const DIOCRGETADDRS: u64 = diowr::<PfiocTable>(70);
/// `DIOCRGETASTATS`.
pub const DIOCRGETASTATS: u64 = diowr::<PfiocTable>(71);
/// `DIOCRCLRASTATS`.
pub const DIOCRCLRASTATS: u64 = diowr::<PfiocTable>(72);
/// `DIOCRTSTADDRS`.
pub const DIOCRTSTADDRS: u64 = diowr::<PfiocTable>(73);
/// `DIOCRSETTFLAGS`.
pub const DIOCRSETTFLAGS: u64 = diowr::<PfiocTable>(74);
/// `DIOCRINADEFINE`.
pub const DIOCRINADEFINE: u64 = diowr::<PfiocTable>(77);
/// `DIOCOSFPFLUSH`.
pub const DIOCOSFPFLUSH: u64 = dio(78);
/// `DIOCOSFPADD`.
pub const DIOCOSFPADD: u64 = diowr::<PfOsfpIoctl>(79);
/// `DIOCOSFPGET`.
pub const DIOCOSFPGET: u64 = diowr::<PfOsfpIoctl>(80);
/// `DIOCXBEGIN`.
pub const DIOCXBEGIN: u64 = diowr::<PfiocTrans>(81);
/// `DIOCXCOMMIT`.
pub const DIOCXCOMMIT: u64 = diowr::<PfiocTrans>(82);
/// `DIOCXROLLBACK`.
pub const DIOCXROLLBACK: u64 = diowr::<PfiocTrans>(83);
/// `DIOCGETSRCNODES`.
pub const DIOCGETSRCNODES: u64 = diowr::<PfiocSrcNodes>(84);
/// `DIOCCLRSRCNODES`.
pub const DIOCCLRSRCNODES: u64 = dio(85);
/// `DIOCSETHOSTID`.
pub const DIOCSETHOSTID: u64 = diowr::<u32>(86);
/// `DIOCIGETIFACES`.
pub const DIOCIGETIFACES: u64 = diowr::<PfiocIface>(87);
/// `DIOCSETIFFLAG`.
pub const DIOCSETIFFLAG: u64 = diowr::<PfiocIface>(89);
/// `DIOCCLRIFFLAG`.
pub const DIOCCLRIFFLAG: u64 = diowr::<PfiocIface>(90);
/// `DIOCKILLSRCNODES`.
pub const DIOCKILLSRCNODES: u64 = diowr::<PfiocSrcNodeKill>(91);
/// `DIOCSETREASS`.
pub const DIOCSETREASS: u64 = diowr::<u32>(92);
/// `DIOCADDQUEUE`.
pub const DIOCADDQUEUE: u64 = diowr::<PfiocQueue>(93);
/// `DIOCGETQUEUES`.
pub const DIOCGETQUEUES: u64 = diowr::<PfiocQueue>(94);
/// `DIOCGETQUEUE`.
pub const DIOCGETQUEUE: u64 = diowr::<PfiocQueue>(95);
/// `DIOCGETQSTATS`.
pub const DIOCGETQSTATS: u64 = diowr::<PfiocQstats>(96);
/// `DIOCSETSYNFLWATS`.
pub const DIOCSETSYNFLWATS: u64 = diowr::<PfiocSynflwats>(97);
/// `DIOCSETSYNCOOKIES`.
pub const DIOCSETSYNCOOKIES: u64 = diowr::<u8>(98);
/// `DIOCGETSYNFLWATS`.
pub const DIOCGETSYNFLWATS: u64 = diowr::<PfiocSynflwats>(99);
/// `DIOCXEND`.
pub const DIOCXEND: u64 = diowr::<u32>(100);
/// `DIOCADDSTATELIM`.
pub const DIOCADDSTATELIM: u64 = diow::<PfiocStatelim>(101);
/// `DIOCADDSOURCELIM`.
pub const DIOCADDSOURCELIM: u64 = diow::<PfiocSourcelim>(102);
/// `DIOCGETSTATELIM`.
pub const DIOCGETSTATELIM: u64 = diowr::<PfiocStatelim>(103);
/// `DIOCGETSOURCELIM`.
pub const DIOCGETSOURCELIM: u64 = diowr::<PfiocSourcelim>(104);
/// `DIOCGETSOURCE`.
pub const DIOCGETSOURCE: u64 = diowr::<PfiocSource>(105);
/// `DIOCGETNSTATELIM`.
pub const DIOCGETNSTATELIM: u64 = diowr::<PfiocStatelim>(106);
/// `DIOCGETNSOURCELIM`.
pub const DIOCGETNSOURCELIM: u64 = diowr::<PfiocSourcelim>(107);
/// `DIOCGETNSOURCE`.
pub const DIOCGETNSOURCE: u64 = diowr::<PfiocSource>(108);
/// `DIOCCLRSOURCE`.
pub const DIOCCLRSOURCE: u64 = diowr::<PfiocSourceKill>(109);

/// `struct pf_pool_limit`: a pool and its configured limit.
pub struct PfPoolLimit {
    /// `pp`: \[I\] the pool.
    pub pp: Option<&'static crate::sys::pool::Pool>,
    /// `limit`: \[p\].
    pub limit: Cell<u32>,
    /// `limit_new`: \[p\].
    pub limit_new: Cell<u32>,
}

// SAFETY: `pp` is immutable after `pfattach`, the limits change under `pf_lock`.
unsafe impl Sync for PfPoolLimit {}

/// The promise `AbiPod` makes, for ABI structures that contain `Cell`s (and so are not
/// `Copy`): see the module docs.
///
/// # Safety
///
/// Implement only for `#[repr(C)]` types made of integers, raw pointer words, `Cell`s of
/// those, arrays and structures of them, without implicit padding (holes are named members),
/// so that every byte is initialised and every bit pattern is a valid value.
pub unsafe trait PfAbi: Sized + 'static {}

/// A pf object that lives in a `pool(9)` and is valid when all its bytes are zero (what the
/// C's `pool_get(..., PR_ZERO)` hands out).
///
/// # Safety
///
/// Implement only for types whose all-zero bit pattern is a valid value: integers, `Cell`s
/// and atomics of them, `Option<&T>`, raw pointers, queue and tree links and heads, `Refcnt`,
/// `Timeout` (initialised before use, as in C) and structures of those.
pub unsafe trait PfPoolItem: Sized + 'static {}

/// `pool_get(pp, flags | PR_ZERO)` of a `T`: a zeroed object, `None` when the pool is empty
/// (`PR_NOWAIT`) or at its limit.
pub fn pf_pool_get<T: PfPoolItem>(pp: &crate::sys::pool::Pool, flags: i32) -> Option<&'static T> {
    let mem = crate::kern::subr_pool::pool_get(pp, flags | crate::sys::pool::PR_ZERO)?;
    kassert!(pp.pr_size.get() as usize >= size_of::<T>());
    // SAFETY: the pool was initialised with items of at least `size_of::<T>()` bytes and the
    // alignment of `T` (`pf_pool_init`), the item is zeroed (`PR_ZERO`), and all-zero is a
    // valid `T` (`PfPoolItem`); the item stays allocated until `pf_pool_put`.
    Some(unsafe { &*mem.as_ptr().cast::<T>() })
}

/// `pool_put(pp, v)`: gives `v` back. The caller has unlinked it from everything that could
/// still reach it, as in C.
pub fn pf_pool_put<T: PfPoolItem>(pp: &crate::sys::pool::Pool, v: &T) {
    crate::kern::subr_pool::pool_put(pp, core::ptr::NonNull::from(v).cast());
}

/// A `pool_get(pp, flags | PR_ZERO)` item that is not published yet: its owner may still
/// write its plain (non-`Cell`) members through [`get_mut`](Self::get_mut), then turns it into
/// the `&'static` kernel object with [`publish`](Self::publish) or gives it back with
/// [`put`](Self::put). Dropping it without either leaks the item (as a C path that forgets
/// it would).
pub struct PfPoolNew<T: PfPoolItem> {
    p: core::ptr::NonNull<T>,
}

impl<T: PfPoolItem> PfPoolNew<T> {
    /// `pool_get(pp, flags | PR_ZERO)`: `None` when the pool is empty (`PR_NOWAIT`) or at
    /// its limit (`PR_LIMITFAIL`).
    pub fn get(pp: &crate::sys::pool::Pool, flags: i32) -> Option<Self> {
        let mem = crate::kern::subr_pool::pool_get(pp, flags | crate::sys::pool::PR_ZERO)?;
        kassert!(pp.pr_size.get() as usize >= size_of::<T>());
        Some(Self { p: mem.cast() })
    }

    /// The item, writable: nothing else can reach it before it is published.
    pub fn get_mut(&mut self) -> &mut T {
        // SAFETY: as for `pf_pool_get` (a zeroed item of at least `size_of::<T>()` bytes with
        // `T`'s alignment, a valid `T`); the pointer is unique to this handle, which the
        // `&mut self` borrow holds.
        unsafe { self.p.as_mut() }
    }

    /// The published kernel object.
    pub fn publish(self) -> &'static T {
        // SAFETY: as for `get_mut`; the handle is consumed, so no `&mut` to the item remains
        // and it stays allocated until `pf_pool_put`.
        unsafe { self.p.as_ref() }
    }

    /// `pool_put(pp, v)` of the unpublished item.
    pub fn put(self, pp: &crate::sys::pool::Pool) {
        crate::kern::subr_pool::pool_put(pp, self.p.cast());
    }
}

impl<T: PfPoolItem> core::ops::Deref for PfPoolNew<T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: as for `get_mut`, shared for the borrow of `self`.
        unsafe { self.p.as_ref() }
    }
}

/// `pool_init(pp, sizeof(T), 0, ipl, flags, wchan, NULL)`.
pub fn pf_pool_init<T: PfPoolItem>(
    pp: &'static crate::sys::pool::Pool,
    ipl: i32,
    flags: i32,
    wchan: &'static str,
) {
    crate::kern::subr_pool::pool_init(
        pp,
        size_of::<T>(),
        core::mem::align_of::<T>() as u32,
        ipl,
        flags,
        wchan,
        None,
    );
}

/// A zeroed `T` on the heap (the ioctl structures are kilobytes; the kernel stack is not).
pub fn pf_abi_zeroed<T: PfAbi>() -> Box<T> {
    // SAFETY: `T: PfAbi`: all-zero bytes are a valid `T`.
    unsafe { Box::<T>::new_zeroed().assume_init() }
}

/// `*(T *)data`: the ioctl argument in the kernel copy `data`, on the heap.
pub fn pf_abi_read<T: PfAbi>(data: &[u8]) -> Box<T> {
    let mut b = Box::<T>::new_zeroed();
    let n = data.len().min(size_of::<T>());
    // SAFETY: `n` bytes fit in both; the destination is the fresh allocation.
    unsafe { ptr::copy_nonoverlapping(data.as_ptr(), b.as_mut_ptr().cast::<u8>(), n) };
    // SAFETY: `T: PfAbi`: any bytes are a valid `T`.
    unsafe { b.assume_init() }
}

/// The bytes of `v`.
pub fn pf_abi_bytes<T: PfAbi>(v: &T) -> &[u8] {
    // SAFETY: `T: PfAbi` has no implicit padding, so every byte is initialised; nothing
    // changes `v` while the slice is borrowed (the callers hold pf's locks or own `v`).
    unsafe { core::slice::from_raw_parts(ptr::from_ref(v).cast::<u8>(), size_of::<T>()) }
}

/// `*(T *)data = *v`: stores `v` in the kernel copy `data` for `sys_ioctl` to copy out.
pub fn pf_abi_write<T: PfAbi>(data: &mut [u8], v: &T) {
    let n = data.len().min(size_of::<T>());
    data[..n].copy_from_slice(&pf_abi_bytes(v)[..n]);
}

/// `copyin(uaddr, &v, sizeof(v))` into a fresh `T` on the heap.
pub fn pf_abi_copyin<T: PfAbi>(uaddr: usize) -> Result<Box<T>, Errno> {
    let mut b = Box::<T>::new_zeroed();
    // SAFETY: the zeroed allocation is a valid `T` viewed as its bytes.
    let buf =
        unsafe { core::slice::from_raw_parts_mut(b.as_mut_ptr().cast::<u8>(), size_of::<T>()) };
    copyin(uaddr, buf)?;
    // SAFETY: `T: PfAbi`: any bytes are a valid `T`.
    Ok(unsafe { b.assume_init() })
}

/// `copyout(&v, uaddr, sizeof(v))`.
pub fn pf_abi_copyout<T: PfAbi>(v: &T, uaddr: usize) -> Result<(), Errno> {
    copyout(pf_abi_bytes(v), uaddr)
}

/// `memcpy(dst, src, sizeof(*dst))` between two `T`s.
pub fn pf_abi_copy_into<T: PfAbi>(dst: &mut T, src: &T) {
    let b = pf_abi_bytes(src);
    // SAFETY: `dst` is a distinct, writable `T`; any bytes are a valid `T` (`PfAbi`).
    unsafe { ptr::copy_nonoverlapping(b.as_ptr(), ptr::from_mut(dst).cast::<u8>(), b.len()) };
}

/// A byte-for-byte copy of `v` on the heap (`bcopy(v, &copy, sizeof(*v))`).
pub fn pf_abi_clone<T: PfAbi>(v: &T) -> Box<T> {
    pf_abi_read(pf_abi_bytes(v))
}

/// The bytes of a C string up to its NUL (or the whole array).
pub fn pf_cstr(b: &[u8]) -> &[u8] {
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    &b[..n]
}

/// `strlcpy(dst, src, sizeof(dst))` over byte arrays.
pub fn pf_strlcpy(dst: &mut [u8], src: &[u8]) -> usize {
    libkern::strlcpy(dst, src)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use core::mem::{offset_of, size_of};

    use super::*;
    use crate::reftest::assert_defines;

    /// The sizes and offsets clang computes for the C header (`-target x86_64-unknown-openbsd
    /// -D_KERNEL`; arm64 gives the same, both are LP64 with natural alignment). pfctl(8) is
    /// compiled against the C structures, so every ioctl argument must match them.
    #[test]
    fn layout() {
        assert_eq!(size_of::<PfRule>(), 1360);
        assert_eq!(size_of::<PfiocRule>(), 3424);
        assert_eq!(size_of::<PfStatus>(), 520);
        assert_eq!(size_of::<PfsyncState>(), 264);
        assert_eq!(size_of::<PfiKif>(), 256);
        assert_eq!(size_of::<PfSrcNode>(), 152);
        assert_eq!(size_of::<PfQueuespec>(), 320);
        assert_eq!(size_of::<PfrAddr>(), 52);
        assert_eq!(size_of::<PfrAstats>(), 160);
        assert_eq!(size_of::<PfrTstats>(), 1232);
        assert_eq!(size_of::<PfrTable>(), 1064);
        assert_eq!(size_of::<PfAddrWrap>(), 48);
        assert_eq!(size_of::<PfPool>(), 136);
        assert_eq!(size_of::<PfRuleAddr>(), 56);
        assert_eq!(size_of::<PfiocTable>(), 1104);
        assert_eq!(size_of::<PfiocIface>(), 40);
        assert_eq!(size_of::<PfiocTrans>(), 16);
        assert_eq!(size_of::<PfiocStates>(), 16);
        assert_eq!(size_of::<PfOsfpIoctl>(), 136);
        assert_eq!(size_of::<PfiocStatelim>(), 128);
        assert_eq!(size_of::<PfiocSourcelim>(), 208);
        assert_eq!(size_of::<PfiocSource>(), 64);
        assert_eq!(size_of::<PfiocSourceEntry>(), 56);
        assert_eq!(size_of::<PfiocStateKill>(), 224);
        assert_eq!(size_of::<PfiocNatlook>(), 80);
        assert_eq!(size_of::<PfiocRuleset>(), 1092);
        assert_eq!(size_of::<PfiocQueue>(), 328);
        assert_eq!(size_of::<PfiocQstats>(), 344);
        assert_eq!(size_of::<PfiocSrcNodeKill>(), 128);
        assert_eq!(size_of::<PfiocTm>(), 8);
        assert_eq!(size_of::<PfiocLimit>(), 8);
        assert_eq!(size_of::<PfiocSynflwats>(), 8);
        assert_eq!(size_of::<PfiocSrcNodes>(), 16);
        assert_eq!(size_of::<PfiocSourceKill>(), 48);
        assert_eq!(size_of::<PfiocTransE>(), 1032);
        assert_eq!(size_of::<PfOsfpEntry>(), 112);
        assert_eq!(size_of::<PfThreshold>(), 16);
        assert_eq!(size_of::<PfsyncStatePeer>(), 32);
        assert_eq!(size_of::<PfsyncStateKey>(), 40);
        assert_eq!(size_of::<PfRuleUid>(), 12);

        assert_eq!(offset_of!(PfRule, skip), 112);
        assert_eq!(offset_of!(PfRule, label), 184);
        assert_eq!(offset_of!(PfRule, entries), 568);
        assert_eq!(offset_of!(PfRule, nat), 584);
        assert_eq!(offset_of!(PfRule, rdr), 720);
        assert_eq!(offset_of!(PfRule, route), 856);
        assert_eq!(offset_of!(PfRule, pktrate), 992);
        assert_eq!(offset_of!(PfRule, evaluations), 1008);
        assert_eq!(offset_of!(PfRule, kif), 1048);
        assert_eq!(offset_of!(PfRule, os_fingerprint), 1080);
        assert_eq!(offset_of!(PfRule, rtableid), 1084);
        assert_eq!(offset_of!(PfRule, timeout), 1092);
        assert_eq!(offset_of!(PfRule, states_cur), 1172);
        assert_eq!(offset_of!(PfRule, max_src_conn_rate), 1200);
        assert_eq!(offset_of!(PfRule, cuid), 1228);
        assert_eq!(offset_of!(PfRule, return_icmp), 1236);
        assert_eq!(offset_of!(PfRule, uid), 1252);
        assert_eq!(offset_of!(PfRule, gid), 1264);
        assert_eq!(offset_of!(PfRule, rule_flag), 1276);
        assert_eq!(offset_of!(PfRule, action), 1280);
        assert_eq!(offset_of!(PfRule, af), 1288);
        assert_eq!(offset_of!(PfRule, type_), 1290);
        assert_eq!(offset_of!(PfRule, flags), 1294);
        assert_eq!(offset_of!(PfRule, flush), 1304);
        assert_eq!(offset_of!(PfRule, naf), 1308);
        assert_eq!(offset_of!(PfRule, rcvifnot), 1309);
        assert_eq!(offset_of!(PfRule, statelim), 1312);
        assert_eq!(offset_of!(PfRule, sourcelim), 1320);
        assert_eq!(offset_of!(PfRule, divert), 1328);
        assert_eq!(offset_of!(PfRule, exptime), 1352);

        assert_eq!(offset_of!(PfPool, key), 48);
        assert_eq!(offset_of!(PfPool, counter), 64);
        assert_eq!(offset_of!(PfPool, ifname), 80);
        assert_eq!(offset_of!(PfPool, kif), 96);
        assert_eq!(offset_of!(PfPool, tblidx), 104);
        assert_eq!(offset_of!(PfPool, states), 112);
        assert_eq!(offset_of!(PfPool, curweight), 120);
        assert_eq!(offset_of!(PfPool, weight), 124);
        assert_eq!(offset_of!(PfPool, opts), 131);

        assert_eq!(offset_of!(PfStatus, since), 440);
        assert_eq!(offset_of!(PfStatus, running), 448);
        assert_eq!(offset_of!(PfStatus, syncookies_active), 480);
        assert_eq!(offset_of!(PfStatus, ifname), 484);
        assert_eq!(offset_of!(PfStatus, pf_chksum), 500);

        assert_eq!(offset_of!(PfiKif, pfik_tree), 16);
        assert_eq!(offset_of!(PfiKif, pfik_packets), 48);
        assert_eq!(offset_of!(PfiKif, pfik_tzero), 176);
        assert_eq!(offset_of!(PfiKif, pfik_flags), 184);
        assert_eq!(offset_of!(PfiKif, pfik_ah_cookie), 192);
        assert_eq!(offset_of!(PfiKif, pfik_states), 216);
        assert_eq!(offset_of!(PfiKif, pfik_dynaddrs), 240);

        assert_eq!(offset_of!(PfSrcNode, addr), 32);
        assert_eq!(offset_of!(PfSrcNode, rule), 64);
        assert_eq!(offset_of!(PfSrcNode, kif), 72);
        assert_eq!(offset_of!(PfSrcNode, bytes), 80);
        assert_eq!(offset_of!(PfSrcNode, states), 112);
        assert_eq!(offset_of!(PfSrcNode, conn_rate), 120);
        assert_eq!(offset_of!(PfSrcNode, creation), 136);
        assert_eq!(offset_of!(PfSrcNode, af), 144);

        assert_eq!(offset_of!(PfsyncState, ifname), 8);
        assert_eq!(offset_of!(PfsyncState, key), 24);
        assert_eq!(offset_of!(PfsyncState, src), 104);
        assert_eq!(offset_of!(PfsyncState, rt_addr), 168);
        assert_eq!(offset_of!(PfsyncState, rule), 184);
        assert_eq!(offset_of!(PfsyncState, packets), 204);
        assert_eq!(offset_of!(PfsyncState, creatorid), 236);
        assert_eq!(offset_of!(PfsyncState, rtableid), 240);
        assert_eq!(offset_of!(PfsyncState, max_mss), 248);
        assert_eq!(offset_of!(PfsyncState, state_flags), 260);
        assert_eq!(offset_of!(PfsyncState, set_prio), 262);

        assert_eq!(offset_of!(PfQueuespec, qname), 16);
        assert_eq!(offset_of!(PfQueuespec, realtime), 160);
        assert_eq!(offset_of!(PfQueuespec, linkshare), 200);
        assert_eq!(offset_of!(PfQueuespec, flowqueue), 280);
        assert_eq!(offset_of!(PfQueuespec, kif), 296);
        assert_eq!(offset_of!(PfQueuespec, flags), 304);
        assert_eq!(offset_of!(PfQueuespec, qid), 312);

        assert_eq!(offset_of!(PfrTstats, pfrts_packets), 1064);
        assert_eq!(offset_of!(PfrTstats, pfrts_match), 1192);
        assert_eq!(offset_of!(PfrTstats, pfrts_tzero), 1208);
        assert_eq!(offset_of!(PfrTstats, pfrts_cnt), 1216);
        assert_eq!(offset_of!(PfrAstats, pfras_packets), 56);
        assert_eq!(offset_of!(PfrAstats, pfras_tzero), 152);

        assert_eq!(offset_of!(PfiocTable, pfrio_buffer), 1064);
        assert_eq!(offset_of!(PfiocTable, pfrio_esize), 1072);
        assert_eq!(offset_of!(PfiocTable, pfrio_ticket), 1100);
        assert_eq!(offset_of!(PfiocTrans, array), 8);
        assert_eq!(offset_of!(PfiocQstats, buf), 328);
        assert_eq!(offset_of!(PfiocQstats, nbytes), 336);
        assert_eq!(offset_of!(PfiocStatelim, limit), 24);
        assert_eq!(offset_of!(PfiocStatelim, description), 36);
        assert_eq!(offset_of!(PfiocStatelim, inuse), 100);
        assert_eq!(offset_of!(PfiocStatelim, admitted), 104);
        assert_eq!(offset_of!(PfiocSourcelim, overload_tblname), 40);
        assert_eq!(offset_of!(PfiocSourcelim, inet_prefix), 80);
        assert_eq!(offset_of!(PfiocSourcelim, description), 88);
        assert_eq!(offset_of!(PfiocSourcelim, nentries), 152);
        assert_eq!(offset_of!(PfiocSourcelim, addrallocs), 160);
        assert_eq!(offset_of!(PfiocSource, entry_size), 32);
        assert_eq!(offset_of!(PfiocSource, key), 40);
        assert_eq!(offset_of!(PfiocSourceEntry, addr), 8);
        assert_eq!(offset_of!(PfiocSourceEntry, inuse), 24);
        assert_eq!(offset_of!(PfiocSourceEntry, admitted), 32);
        assert_eq!(offset_of!(PfiocSourceKill, af), 24);
        assert_eq!(offset_of!(PfiocSourceKill, addr), 28);
        assert_eq!(offset_of!(PfiocSourceKill, rmstates), 44);
        assert_eq!(offset_of!(PfiocStateKill, psk_af), 16);
        assert_eq!(offset_of!(PfiocStateKill, psk_proto), 20);
        assert_eq!(offset_of!(PfiocStateKill, psk_src), 24);
        assert_eq!(offset_of!(PfiocStateKill, psk_ifname), 136);
        assert_eq!(offset_of!(PfiocStateKill, psk_killed), 216);
        assert_eq!(offset_of!(PfiocStateKill, psk_rdomain), 220);
        assert_eq!(offset_of!(PfiocSrcNodeKill, psnk_src), 8);
        assert_eq!(offset_of!(PfiocSrcNodeKill, psnk_killed), 120);
        assert_eq!(offset_of!(PfOsfpIoctl, fp_tcpopts), 112);
        assert_eq!(offset_of!(PfOsfpIoctl, fp_getnum), 132);
        assert_eq!(offset_of!(PfiocQueue, queue), 8);
        assert_eq!(offset_of!(PfiocIface, pfiio_buffer), 16);
        assert_eq!(offset_of!(PfiocRule, rule), 2064);
    }

    /// The ioctl numbers encode the structure sizes; a few spelled out from the C's `_IOWR`.
    #[test]
    fn ioctl_numbers() {
        use crate::sys::ioccom::{IOC_INOUT, IOC_VOID};
        let iowr =
            |n: u64, len: u64| IOC_INOUT | ((len & 0x1fff) << 16) | (u64::from(b'D') << 8) | n;
        assert_eq!(DIOCSTART, IOC_VOID | (u64::from(b'D') << 8) | 1);
        assert_eq!(DIOCSTOP, IOC_VOID | (u64::from(b'D') << 8) | 2);
        assert_eq!(DIOCADDRULE, iowr(4, 3424));
        assert_eq!(DIOCGETSTATUS, iowr(21, 520));
        assert_eq!(DIOCXBEGIN, iowr(81, 16));
        assert_eq!(DIOCRADDTABLES, iowr(61, 1104));
        assert_eq!(DIOCIGETIFACES, iowr(87, 40));
    }

    #[test]
    fn addresses() {
        let mut a = PfAddr::zeroed();
        a.set_v4(crate::netinet::in_::InAddr {
            s_addr: 0x0a00_0202u32.to_be(),
        });
        assert_eq!(a.addr8[..4], [10, 0, 2, 2]);
        assert_eq!(u32::from_be(a.addr32(0)), 0x0a00_0202);
        assert_eq!(u16::from_be(a.addr16(1)), 0x0202);
        let b = a;
        assert!(pf_aeq(&a, &b, crate::sys::socket::AF_INET));
        assert!(!pf_aneq(&a, &b, crate::sys::socket::AF_INET));
        assert!(pf_azero(&PfAddr::zeroed(), crate::sys::socket::AF_INET));
        assert!(pf_pool_dyntype(PF_POOL_ROUNDROBIN | PF_POOL_STICKYADDR));
        assert!(!pf_pool_dyntype(PF_POOL_BITMASK));
        let packed = pf_osfp_pack(5, 6, 7);
        assert_eq!(pf_osfp_unpack(packed), (5, 6, 7));
        assert_eq!(PF_OSFP_MAX_OPTS, 21);
    }

    #[test]
    fn abi_copies() {
        let mut data = [0u8; 3424];
        let mut pr: alloc::boxed::Box<PfiocRule> = pf_abi_zeroed();
        pr.ticket = 7;
        pr.rule.action = PF_DROP;
        pr.rule.nr.set(3);
        pf_abi_write(&mut data, &*pr);
        let back: alloc::boxed::Box<PfiocRule> = pf_abi_read(&data);
        assert_eq!(back.ticket, 7);
        assert_eq!(back.rule.action, PF_DROP);
        assert_eq!(back.rule.nr.get(), 3);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/net/pfvar.h");
        assert_defines!(defs;
        PF_MD5_DIGEST_LENGTH, PFTM_TCP_FIRST_PACKET_VAL, PFTM_TCP_OPENING_VAL,
        PFTM_TCP_FIN_WAIT_VAL, PFTM_TCP_CLOSED_VAL, PFTM_UDP_FIRST_PACKET_VAL,
        PFTM_UDP_SINGLE_VAL, PFTM_UDP_MULTIPLE_VAL, PFTM_ICMP_FIRST_PACKET_VAL,
        PFTM_ICMP_ERROR_REPLY_VAL, PFTM_OTHER_FIRST_PACKET_VAL, PFTM_OTHER_SINGLE_VAL,
        PFTM_OTHER_MULTIPLE_VAL, PFTM_FRAG_VAL, PFTM_INTERVAL_VAL, PFTM_SRC_NODE_VAL,
        PFTM_TS_DIFF_VAL, PF_FRAG_STALE, PF_FRAG_ENTRY_POINTS, PF_FRAG_ENTRY_LIMIT,
        PF_POOL_IDMASK, PF_POOL_TYPEMASK, PF_POOL_STICKYADDR, PF_WSCALE_FLAG, PF_WSCALE_MASK,
        PF_LOG, PF_LOG_ALL, PF_LOG_USER, PF_LOG_FORCE, PF_LOG_MATCHES, PF_TABLE_NAME_SIZE,
        PFI_AFLAG_NETWORK, PFI_AFLAG_BROADCAST, PFI_AFLAG_PEER, PFI_AFLAG_MODEMASK,
        PFI_AFLAG_NOALIAS, PF_THRESHOLD_MULT, PF_OSFP_EXPANDED, PF_OSFP_GENERIC,
        PF_OSFP_NODETAIL, PF_OSFP_LEN, PF_OSFP_WSIZE_MOD, PF_OSFP_WSIZE_DC, PF_OSFP_WSIZE_MSS,
        PF_OSFP_WSIZE_MTU, PF_OSFP_PSIZE_MOD, PF_OSFP_PSIZE_DC, PF_OSFP_WSCALE,
        PF_OSFP_WSCALE_MOD, PF_OSFP_WSCALE_DC, PF_OSFP_MSS, PF_OSFP_MSS_MOD, PF_OSFP_MSS_DC,
        PF_OSFP_DF, PF_OSFP_TS0, PF_OSFP_INET6, PF_OSFP_MAXTTL_OFFSET, PF_OSFP_TCPOPT_NOP,
        PF_OSFP_TCPOPT_WSCALE, PF_OSFP_TCPOPT_MSS, PF_OSFP_TCPOPT_SACK, PF_OSFP_TCPOPT_TS,
        PF_OSFP_TCPOPT_BITS, PF_ANCHOR_STACK_MAX, PF_ANCHOR_NAME_SIZE, PF_ANCHOR_HIWAT,
        PF_SKIP_IFP, PF_SKIP_DIR, PF_SKIP_RDOM, PF_SKIP_AF, PF_SKIP_PROTO, PF_SKIP_SRC_ADDR,
        PF_SKIP_DST_ADDR, PF_SKIP_SRC_PORT, PF_SKIP_DST_PORT, PF_SKIP_COUNT,
        PF_RULE_LABEL_SIZE, PF_QNAME_SIZE, PF_TAG_NAME_SIZE, PF_STATE_NORMAL,
        PF_STATE_MODULATE, PF_STATE_SYNPROXY, PF_FLUSH, PF_FLUSH_GLOBAL, PFRULE_DROP,
        PFRULE_RETURNRST, PFRULE_FRAGMENT, PFRULE_RETURNICMP, PFRULE_RETURN, PFRULE_NOSYNC,
        PFRULE_SRCTRACK, PFRULE_RULESRCTRACK, PFRULE_SETDELAY, PFRULE_IFBOUND,
        PFRULE_STATESLOPPY, PFRULE_PFLOW, PFRULE_ONCE, PFRULE_AFTO, PFRULE_EXPIRED,
        PFSTATE_HIWAT, PFSTATE_ADAPT_START, PFSTATE_ADAPT_END, PF_PKTDELAY_MAXPKTS,
        PFSNODE_HIWAT, PFSS_TIMESTAMP, PFSS_PAWS, PFSS_PAWS_IDLED, PFSS_DATA_TS,
        PFSS_DATA_NOTS, PFSTATE_ALLOWOPTS, PFSTATE_SLOPPY, PFSTATE_PFLOW, PFSTATE_NOSYNC,
        PFSTATE_ACK, PFSTATE_NODF, PFSTATE_SETTOS, PFSTATE_RANDOMID, PFSTATE_SCRUB_TCP,
        PFSTATE_SETPRIO, PFSTATE_INP_UNLINKED, PFSYNC_SCRUB_FLAG_VALID, PFSYNC_FLAG_SRCNODE,
        PFSYNC_FLAG_NATSRCNODE, PFR_TFLAG_PERSIST, PFR_TFLAG_CONST, PFR_TFLAG_ACTIVE,
        PFR_TFLAG_INACTIVE, PFR_TFLAG_REFERENCED, PFR_TFLAG_REFDANCHOR, PFR_TFLAG_COUNTERS,
        PFR_TFLAG_USRMASK, PFR_TFLAG_SETMASK, PFR_TFLAG_ALLMASK, PFRKE_FLAG_NOT,
        PFRKE_FLAG_MARK, PFI_IFLAG_SKIP, PFI_IFLAG_ANY, PF_DPORT_RANGE, PF_RPORT_RANGE,
        PFRES_MATCH, PFRES_BADOFF, PFRES_FRAG, PFRES_SHORT, PFRES_NORM, PFRES_MEMORY,
        PFRES_TS, PFRES_CONGEST, PFRES_IPOPTIONS, PFRES_PROTCKSUM, PFRES_BADSTATE,
        PFRES_STATEINS, PFRES_MAXSTATES, PFRES_SRCLIMIT, PFRES_SYNPROXY, PFRES_TRANSLATE,
        PFRES_NOROUTE, PFRES_MAX, LCNT_STATES, LCNT_SRCSTATES, LCNT_SRCNODES, LCNT_SRCCONN,
        LCNT_SRCCONNRATE, LCNT_OVERLOAD_TABLE, LCNT_OVERLOAD_FLUSH, LCNT_SYNFLOODS,
        LCNT_SYNCOOKIES_SENT, LCNT_SYNCOOKIES_VALID, LCNT_MAX, PFUDPS_NO_TRAFFIC,
        PFUDPS_SINGLE, PFUDPS_MULTIPLE, PFUDPS_NSTATES, PFOTHERS_NO_TRAFFIC, PFOTHERS_SINGLE,
        PFOTHERS_MULTIPLE, PFOTHERS_NSTATES, FCNT_STATE_SEARCH, FCNT_STATE_INSERT,
        FCNT_STATE_REMOVALS, FCNT_MAX, SCNT_SRC_NODE_SEARCH, SCNT_SRC_NODE_INSERT,
        SCNT_SRC_NODE_REMOVALS, SCNT_MAX, NCNT_FRAG_SEARCH, NCNT_FRAG_INSERT,
        NCNT_FRAG_REMOVALS, NCNT_MAX, PF_REASS_ENABLED, PF_REASS_NODF, PF_SYNCOOKIES_NEVER,
        PF_SYNCOOKIES_ALWAYS, PF_SYNCOOKIES_ADAPTIVE, PF_SYNCOOKIES_HIWATPCT, PF_PRIO_ZERO,
        PFQS_FLOWQUEUE, PFQS_ROOTCLASS, PFQS_DEFAULT, PFR_KTABLE_HIWAT, PFR_KENTRY_HIWAT,
        PFR_KENTRY_HIWAT_SMALL, PFR_FLAG_DUMMY, PFR_FLAG_FEEDBACK, PFR_FLAG_CLSTATS,
        PFR_FLAG_ADDRSTOO, PFR_FLAG_REPLACE, PFR_FLAG_ALLRSETS, PFR_FLAG_ALLMASK,
        PFR_FLAG_USERIOCTL, PF_STATELIM_NAME_LEN, PF_STATELIM_DESCR_LEN, PF_STATELIM_ID_NONE,
        PF_STATELIM_ID_MIN, PF_STATELIM_ID_MAX, PF_STATELIM_LIMIT_MIN, PF_SOURCELIM_NAME_LEN,
        PF_SOURCELIM_DESCR_LEN, PF_SOURCELIM_ID_NONE, PF_SOURCELIM_ID_MIN,
        PF_SOURCELIM_ID_MAX);
        // Spelled as expressions in the C.
        assert_eq!(PFTM_TCP_ESTABLISHED_VAL, 24 * 60 * 60);
        assert_eq!(PFTM_TCP_CLOSING_VAL, 15 * 60);
        assert_eq!(PF_STATELIM_LIMIT_MAX, 1 << 24);
    }
}
/* </TESTS> */
