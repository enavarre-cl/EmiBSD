/*	$OpenBSD: mbuf.h,v 1.271 2026/07/03 11:51:57 dlg Exp $	*/
/*	$NetBSD: mbuf.h,v 1.19 1996/02/09 18:25:14 christos Exp $	*/
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
 *	@(#)mbuf.h	8.5 (Berkeley) 2/19/95
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/mbuf.h>`: memory buffers, the network stack's packets.
//!
//! Upstream: sys/sys/mbuf.h @ 3ce1f3f79392
//!
//! An mbuf is `MSIZE` bytes: a header ([`MHdr`]) and a data area. A plain mbuf keeps up to
//! [`MLEN`] bytes of data in that area; the first mbuf of a packet (`M_PKTHDR`) keeps a packet
//! header ([`Pkthdr`]) at the start of the area and [`MHLEN`] bytes after it; an mbuf with
//! external storage (`M_EXT`, a cluster) keeps the storage's description ([`MbufExt`]) where
//! the packet data would be and points `m_data` into the cluster. The functions live in
//! `kern/uipc_mbuf.rs` and `kern/uipc_mbuf2.rs`; the header's inline functions and macros
//! are here.
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - `struct mbuf *` is `&'static Mbuf` (an item of `mbpool`, valid until `m_free`) and NULL is
//!   `None`; the header's fields are `Cell`s, so mbufs are changed through shared references
//!   as the C changes them through pointers. The `m_next`/`m_len`/... macros are methods that
//!   return the field's `Cell`.
//! - The `M_dat` union is one byte area, [`Mbuf::m_dat`]; `m_pkthdr` and `m_ext` are views of
//!   its start and of the bytes after the packet header ([`Mbuf::m_pkthdr`], [`Mbuf::m_ext`]),
//!   as the union's members are. Structure assignments of those views are byte copies inside
//!   the area ([`Mbuf::m_pkthdr_assign`], [`Mbuf::m_ext_assign`]).
//! - `MGET`, `MGETHDR` and `M_PREPEND` only assign the result of `m_get`, `m_gethdr` and
//!   `m_prepend` to their argument; Rust callers write the assignment.
//! - `MCLREFDEBUGN`/`MCLREFDEBUGO` (`option DEBUG`) record the caller's location through
//!   `#[track_caller]` instead of `__FILE__`/`__LINE__`.
//! - `struct pf_state_key *` and `struct inpcb *` in `struct pkthdr_pf` are untyped pointers
//!   until pf(4) and `netinet` are ported.
//! - `mbstat_inc` bumps one array of atomics: `struct cpumem` (`<sys/percpu.h>`) is not ported,
//!   and there is one CPU (see `kern/uipc_mbuf.rs`).
//! - `mq_len`'s `READ_ONCE` is a relaxed atomic load: `ml_len` is an `AtomicU32`.
//! - `m_extreferenced` answers `false` for an mbuf without `M_EXT` instead of reading a stale
//!   `m_ext` (the C only asks with `M_EXT` set, through `M_READONLY`).

use core::cell::{Cell, UnsafeCell};
use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::kern::uipc_mbuf::{M_EXTFREE_REFS_FN, m_clget, m_ext_refs_shared, m_freem, mbstat};
use crate::machine::intr::{splnet, splx};
use crate::queue_adapter;
use crate::sys::malloc::{M_NOWAIT, M_WAITOK};
use crate::sys::mutex::Mutex;
use crate::sys::percpu::counters_inc;
use crate::sys::queue::{SlistEntry, SlistHead};

// Constants related to network buffer management. MCLBYTES must be no larger than PAGE_SIZE
// (the software page size) and, on machines that exchange pages of input or output buffers
// with mbuf clusters (MAPPED_MBUFS), MCLBYTES must also be an integral multiple of the hardware
// page size.

/// `MSIZE`: size of an mbuf.
pub const MSIZE: usize = 256;

// Mbufs are of a single size, MSIZE, which includes overhead. An mbuf may add a single "mbuf
// cluster" of size MCLBYTES, which has no additional overhead and is used instead of the
// internal data area; this is done when at least MINCLSIZE of data must be stored.

/// `MLEN`: normal data len.
pub const MLEN: usize = MSIZE - size_of::<MHdr>();
/// `MHLEN`: data len w/pkthdr.
pub const MHLEN: usize = MLEN - size_of::<Pkthdr>();

/// `MAXMCLBYTES`: largest cluster from the stack.
pub const MAXMCLBYTES: usize = 64 * 1024;
/// `MINCLSIZE`: smallest amount to put in cluster.
pub const MINCLSIZE: usize = MHLEN + MLEN + 1;

/// `MCLSHIFT`: convert bytes to m_buf clusters; a 2K cluster can hold an Ethernet frame.
pub const MCLSHIFT: usize = 11;
/// `MCLBYTES`: size of a m_buf cluster.
pub const MCLBYTES: usize = 1 << MCLSHIFT;

// pkthdr_pf.flags

/// `PF_TAG_GENERATED`.
pub const PF_TAG_GENERATED: u8 = 0x01;
/// `PF_TAG_SYNCOOKIE_RECREATED`.
pub const PF_TAG_SYNCOOKIE_RECREATED: u8 = 0x02;
/// `PF_TAG_TRANSLATE_LOCALHOST`.
pub const PF_TAG_TRANSLATE_LOCALHOST: u8 = 0x04;
/// `PF_TAG_DIVERTED`.
pub const PF_TAG_DIVERTED: u8 = 0x08;
/// `PF_TAG_DIVERTED_PACKET`.
pub const PF_TAG_DIVERTED_PACKET: u8 = 0x10;
/// `PF_TAG_REROUTE`.
pub const PF_TAG_REROUTE: u8 = 0x20;
/// `PF_TAG_REFRAGMENTED`: refragmented ipv6 packet.
pub const PF_TAG_REFRAGMENTED: u8 = 0x40;
/// `PF_TAG_PROCESSED`: packet was checked by pf.
pub const PF_TAG_PROCESSED: u8 = 0x80;

/// `MPF_BITS`: the `%b` description of `pkthdr_pf.flags`.
pub const MPF_BITS: &[u8] = b"\x10\x01GENERATED\x02SYNCOOKIE_RECREATED\x03TRANSLATE_LOCALHOST\
\x04DIVERTED\x05DIVERTED_PACKET\x06REROUTE\x07REFRAGMENTED\x08PROCESSED";

// mbuf flags

/// `M_EXT`: has associated external storage.
pub const M_EXT: u16 = 0x0001;
/// `M_PKTHDR`: start of record.
pub const M_PKTHDR: u16 = 0x0002;
/// `M_EOR`: end of record.
pub const M_EOR: u16 = 0x0004;
/// `M_EXTWR`: external storage is writable.
pub const M_EXTWR: u16 = 0x0008;
/// `M_PROTO1`: protocol-specific.
pub const M_PROTO1: u16 = 0x0010;

// mbuf pkthdr flags, also in m_flags

/// `M_VLANTAG`: `ether_vtag` is valid.
pub const M_VLANTAG: u16 = 0x0020;
/// `M_LOOP`: packet has been sent from local machine.
pub const M_LOOP: u16 = 0x0040;
/// `M_BCAST`: sent/received as link-level broadcast.
pub const M_BCAST: u16 = 0x0100;
/// `M_MCAST`: sent/received as link-level multicast.
pub const M_MCAST: u16 = 0x0200;
/// `M_CONF`: payload was encrypted (ESP-transport).
pub const M_CONF: u16 = 0x0400;
/// `M_AUTH`: payload was authenticated (AH or ESP auth).
pub const M_AUTH: u16 = 0x0800;
/// `M_TUNNEL`: IP-in-IP added by tunnel mode IPsec.
pub const M_TUNNEL: u16 = 0x1000;
/// `M_ZEROIZE`: zeroize data part on free.
pub const M_ZEROIZE: u16 = 0x2000;
/// `M_COMP`: header was decompressed.
pub const M_COMP: u16 = 0x4000;
/// `M_LINK0`: link layer specific flag.
pub const M_LINK0: u16 = 0x8000;

/// `M_BITS`: the `%b` description of `m_flags`.
pub const M_BITS: &[u8] = b"\x10\x01M_EXT\x02M_PKTHDR\x03M_EOR\x04M_EXTWR\x05M_PROTO1\
\x06M_VLANTAG\x07M_LOOP\x09M_BCAST\x0aM_MCAST\x0bM_CONF\x0cM_AUTH\x0dM_TUNNEL\x0eM_ZEROIZE\
\x0fM_COMP\x10M_LINK0";

/// `M_COPYFLAGS`: flags copied when copying `m_pkthdr`.
pub const M_COPYFLAGS: u16 = M_PKTHDR
    | M_EOR
    | M_PROTO1
    | M_BCAST
    | M_MCAST
    | M_CONF
    | M_COMP
    | M_AUTH
    | M_LOOP
    | M_TUNNEL
    | M_LINK0
    | M_VLANTAG
    | M_ZEROIZE;

// Checksumming flags

/// `M_IPV4_CSUM_OUT`: IPv4 checksum needed.
pub const M_IPV4_CSUM_OUT: u16 = 0x0001;
/// `M_TCP_CSUM_OUT`: TCP checksum needed.
pub const M_TCP_CSUM_OUT: u16 = 0x0002;
/// `M_UDP_CSUM_OUT`: UDP checksum needed.
pub const M_UDP_CSUM_OUT: u16 = 0x0004;
/// `M_IPV4_CSUM_IN_OK`: IPv4 checksum verified.
pub const M_IPV4_CSUM_IN_OK: u16 = 0x0008;
/// `M_IPV4_CSUM_IN_BAD`: IPv4 checksum bad.
pub const M_IPV4_CSUM_IN_BAD: u16 = 0x0010;
/// `M_TCP_CSUM_IN_OK`: TCP checksum verified.
pub const M_TCP_CSUM_IN_OK: u16 = 0x0020;
/// `M_TCP_CSUM_IN_BAD`: TCP checksum bad.
pub const M_TCP_CSUM_IN_BAD: u16 = 0x0040;
/// `M_UDP_CSUM_IN_OK`: UDP checksum verified.
pub const M_UDP_CSUM_IN_OK: u16 = 0x0080;
/// `M_UDP_CSUM_IN_BAD`: UDP checksum bad.
pub const M_UDP_CSUM_IN_BAD: u16 = 0x0100;
/// `M_ICMP_CSUM_OUT`: ICMP/ICMPv6 checksum needed.
pub const M_ICMP_CSUM_OUT: u16 = 0x0200;
/// `M_ICMP_CSUM_IN_OK`: ICMP/ICMPv6 checksum verified.
pub const M_ICMP_CSUM_IN_OK: u16 = 0x0400;
/// `M_ICMP_CSUM_IN_BAD`: ICMP/ICMPv6 checksum bad.
pub const M_ICMP_CSUM_IN_BAD: u16 = 0x0800;
/// `M_IPV6_DF_OUT`: don't fragment outgoing IPv6.
pub const M_IPV6_DF_OUT: u16 = 0x1000;
/// `M_TIMESTAMP`: `ph_timestamp` is set.
pub const M_TIMESTAMP: u16 = 0x2000;
/// `M_FLOWID`: `ph_flowid` is set.
pub const M_FLOWID: u16 = 0x4000;
/// `M_TCP_TSO`: TCP Segmentation Offload needed.
pub const M_TCP_TSO: u16 = 0x8000;

/// `MCS_BITS`: the `%b` description of `csum_flags`.
pub const MCS_BITS: &[u8] = b"\x10\x01IPV4_CSUM_OUT\x02TCP_CSUM_OUT\x03UDP_CSUM_OUT\
\x04IPV4_CSUM_IN_OK\x05IPV4_CSUM_IN_BAD\x06TCP_CSUM_IN_OK\x07TCP_CSUM_IN_BAD\x08UDP_CSUM_IN_OK\
\x09UDP_CSUM_IN_BAD\x0aICMP_CSUM_OUT\x0bICMP_CSUM_IN_OK\x0cICMP_CSUM_IN_BAD\x0dIPV6_NODF_OUT\
\x0eTIMESTAMP\x0fFLOWID\x10TCP_TSO";

// mbuf types

/// `MT_FREE`: should be on free list.
pub const MT_FREE: i32 = 0;
/// `MT_DATA`: dynamic (data) allocation.
pub const MT_DATA: i32 = 1;
/// `MT_HEADER`: packet header.
pub const MT_HEADER: i32 = 2;
/// `MT_SONAME`: socket name.
pub const MT_SONAME: i32 = 3;
/// `MT_SOOPTS`: socket options.
pub const MT_SOOPTS: i32 = 4;
/// `MT_FTABLE`: fragment reassembly header.
pub const MT_FTABLE: i32 = 5;
/// `MT_CONTROL`: extra-data protocol message.
pub const MT_CONTROL: i32 = 6;
/// `MT_OOBDATA`: expedited data.
pub const MT_OOBDATA: i32 = 7;
/// `MT_NTYPES`.
pub const MT_NTYPES: usize = 8;

// flags to m_get/MGET

/// `M_DONTWAIT`: `M_NOWAIT`.
pub const M_DONTWAIT: i32 = M_NOWAIT;
/// `M_WAIT`: `M_WAITOK`.
pub const M_WAIT: i32 = M_WAITOK;

/// `MEXTFREE_POOL`: the index of `m_extfree_pool`, the first free function registered.
pub const MEXTFREE_POOL: u32 = 0;

/// `M_COPYALL`: length to `m_copym` to copy all.
pub const M_COPYALL: i32 = 1_000_000_000;

// Packet tag types

/// `PACKET_TAG_IPSEC_IN_DONE`: IPsec applied, in.
pub const PACKET_TAG_IPSEC_IN_DONE: u16 = 0x0001;
/// `PACKET_TAG_IPSEC_OUT_DONE`: IPsec applied, out.
pub const PACKET_TAG_IPSEC_OUT_DONE: u16 = 0x0002;
/// `PACKET_TAG_IPSEC_FLOWINFO`: IPsec flowinfo.
pub const PACKET_TAG_IPSEC_FLOWINFO: u16 = 0x0004;
/// `PACKET_TAG_IP_OFFNXT`: IPv4 offset and next proto.
pub const PACKET_TAG_IP_OFFNXT: u16 = 0x0010;
/// `PACKET_TAG_IP6_OFFNXT`: IPv6 offset and next proto.
pub const PACKET_TAG_IP6_OFFNXT: u16 = 0x0020;
/// `PACKET_TAG_WIREGUARD`: WireGuard data.
pub const PACKET_TAG_WIREGUARD: u16 = 0x0040;
/// `PACKET_TAG_GRE`: GRE processing done.
pub const PACKET_TAG_GRE: u16 = 0x0080;
/// `PACKET_TAG_DLT`: data link layer type.
pub const PACKET_TAG_DLT: u16 = 0x0100;
/// `PACKET_TAG_PF_DIVERT`: pf(4) diverted packet.
pub const PACKET_TAG_PF_DIVERT: u16 = 0x0200;
/// `PACKET_TAG_PF_REASSEMBLED`: pf reassembled ipv6 packet.
pub const PACKET_TAG_PF_REASSEMBLED: u16 = 0x0800;
/// `PACKET_TAG_SRCROUTE`: IPv4 source routing options.
pub const PACKET_TAG_SRCROUTE: u16 = 0x1000;
/// `PACKET_TAG_TUNNEL`: tunnel endpoint address.
pub const PACKET_TAG_TUNNEL: u16 = 0x2000;
/// `PACKET_TAG_CARP_BAL_IP`: carp(4) ip balanced marker.
pub const PACKET_TAG_CARP_BAL_IP: u16 = 0x4000;

/// `MTAG_BITS`: the `%b` description of `ph_tagsset`.
pub const MTAG_BITS: &[u8] = b"\x10\x01IPSEC_IN_DONE\x02IPSEC_OUT_DONE\x03IPSEC_FLOWINFO\
\x04IPSEC_OUT_CRYPTO_NEEDED\x05IPSEC_PENDING_TDB\x06BRIDGE\x07WG\x08GRE\x09DLT\x0aPF_DIVERT\
\x0cPF_REASSEMBLED\x0dSRCROUTE\x0eTUNNEL\x0fCARP_BAL_IP";

/// `PACKET_TAG_MAXSIZE`: maximum tag payload length (that is excluding the m_tag structure).
/// Please make sure to update this value when increasing the payload length for an existing
/// packet tag type or when adding a new one that has payload larger than the value below.
pub const PACKET_TAG_MAXSIZE: usize = 80;

/// `M_MAXLOOP`: detect mbufs looping in the kernel when spliced too often.
pub const M_MAXLOOP: u8 = 128;

/// `struct m_tag`: a packet tag; its `m_tag_len` bytes of data follow the structure.
#[repr(C)]
pub struct MTag {
    /// `m_tag_link`: list of packet tags.
    pub m_tag_link: SlistEntry<MTag>,
    /// `m_tag_id`: tag ID.
    pub m_tag_id: Cell<u16>,
    /// `m_tag_len`: length of data.
    pub m_tag_len: Cell<u16>,
}

impl MTag {
    /// A tag of `id` with `len` bytes of data, on no list.
    pub const fn new(id: u16, len: u16) -> Self {
        Self {
            m_tag_link: SlistEntry::new(),
            m_tag_id: Cell::new(id),
            m_tag_len: Cell::new(len),
        }
    }

    /// `(t + 1)`: the tag's data, right after the structure in the same `mtagpool` item.
    pub fn data(&self) -> *mut u8 {
        ptr::from_ref(self).wrapping_add(1).cast::<u8>().cast_mut()
    }
}

queue_adapter!(
    /// `SLIST_HEAD(, m_tag)` through `m_tag_link`: a packet's tags.
    pub MTagList: MTag, m_tag_link => SlistEntry<MTag>
);

/// `struct m_hdr`: header at beginning of each mbuf.
#[repr(C)]
pub struct MHdr {
    /// `mh_next`: next buffer in chain.
    pub mh_next: Cell<Option<&'static Mbuf>>,
    /// `mh_nextpkt`: next chain in queue/record.
    pub mh_nextpkt: Cell<Option<&'static Mbuf>>,
    /// `mh_data`: location of data.
    pub mh_data: Cell<*mut u8>,
    /// `mh_len`: amount of data in this mbuf.
    pub mh_len: Cell<u32>,
    /// `mh_type`: type of data in this mbuf.
    pub mh_type: Cell<i16>,
    /// `mh_flags`: flags; see below.
    pub mh_flags: Cell<u16>,
}

impl MHdr {
    /// A header with every field zero, as a fresh mbuf's is before `m_get` fills it.
    pub const fn new() -> Self {
        Self {
            mh_next: Cell::new(None),
            mh_nextpkt: Cell::new(None),
            mh_data: Cell::new(ptr::null_mut()),
            mh_len: Cell::new(0),
            mh_type: Cell::new(0),
            mh_flags: Cell::new(0),
        }
    }
}

impl Default for MHdr {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct pkthdr_pf`: pf stuff.
#[repr(C)]
pub struct PkthdrPf {
    /// `statekey`: pf stackside statekey (`struct pf_state_key *`).
    pub statekey: Cell<*mut ()>,
    /// `inp`: connected pcb for outgoing packet (`struct inpcb *`).
    pub inp: Cell<*mut ()>,
    /// `qid`: queue id.
    pub qid: Cell<u32>,
    /// `tag`: tag id.
    pub tag: Cell<u16>,
    /// `delay`: delay packet by X ms.
    pub delay: Cell<u16>,
    /// `flags`: `PF_TAG_*`.
    pub flags: Cell<u8>,
    /// `routed`.
    pub routed: Cell<u8>,
    /// `prio`.
    pub prio: Cell<u8>,
    /// `pad`.
    pub pad: Cell<[u8; 1]>,
}

/// `struct pkthdr`: record/packet header in first mbuf of chain; valid if `M_PKTHDR` set.
#[repr(C)]
pub struct Pkthdr {
    /// `ph_cookie`: additional data.
    pub ph_cookie: Cell<*mut ()>,
    /// `ph_tags`: list of packet tags.
    pub ph_tags: SlistHead<MTagList>,
    /// `ph_timestamp`: packet timestamp.
    pub ph_timestamp: Cell<i64>,
    /// `len`: total packet length.
    pub len: Cell<i32>,
    /// `ph_rtableid`: routing table id.
    pub ph_rtableid: Cell<u32>,
    /// `ph_ifidx`: rcv interface index.
    pub ph_ifidx: Cell<u32>,
    /// `ph_tagsset`: mtags attached.
    pub ph_tagsset: Cell<u16>,
    /// `ph_flowid`: pseudo unique flow id.
    pub ph_flowid: Cell<u16>,
    /// `csum_flags`: checksum flags.
    pub csum_flags: Cell<u16>,
    /// `ether_vtag`: Ethernet 802.1p+Q vlan tag.
    pub ether_vtag: Cell<u16>,
    /// `ph_mss`: TCP max segment size.
    pub ph_mss: Cell<u16>,
    /// `ph_loopcnt`: mbuf is looping in kernel.
    pub ph_loopcnt: Cell<u8>,
    /// `ph_family`: af, used when queueing.
    pub ph_family: Cell<u8>,
    /// `pf`.
    pub pf: PkthdrPf,
}

/// `struct mbuf_ext`: description of external storage mapped into mbuf, valid if `M_EXT` set.
#[repr(C)]
pub struct MbufExt {
    /// `ext_buf`: start of buffer.
    pub ext_buf: Cell<*mut u8>,
    /// `ext_arg`: the free function's argument.
    pub ext_arg: Cell<*mut ()>,
    /// `ext_free_fn`: index of free function.
    pub ext_free_fn: Cell<u32>,
    /// `ext_size`: size of buffer, for `ext_free_fn`.
    pub ext_size: Cell<u32>,
    /// `ext_ofile` (`DEBUG`): a `&'static str` kept as a raw pointer, so the view stays valid
    /// for any bytes.
    #[cfg(feature = "debug")]
    pub ext_ofile: Cell<*const str>,
    /// `ext_nfile` (`DEBUG`), as `ext_ofile`.
    #[cfg(feature = "debug")]
    pub ext_nfile: Cell<*const str>,
    /// `ext_oline` (`DEBUG`).
    #[cfg(feature = "debug")]
    pub ext_oline: Cell<i32>,
    /// `ext_nline` (`DEBUG`).
    #[cfg(feature = "debug")]
    pub ext_nline: Cell<i32>,
}

/// `void (*)(caddr_t, u_int, void *)`: an external storage free function (`MEXTADD`'s
/// `freefn`, registered with `mextfree_register`); it gets the buffer, its size and the
/// argument `MEXTADD` stored.
///
/// # Safety
///
/// Called only with the buffer, size and argument stored together with the function's index.
pub type MextFreeFn = unsafe fn(*mut u8, u32, *mut ());

/// The `M_dat` union's bytes: `M_databuf`, or `MH_pkthdr` followed by `MH_ext` or
/// `MH_databuf`.
#[repr(C, align(8))]
pub struct MDat(pub [u8; MLEN]);

/// `struct mbuf`.
#[repr(C)]
pub struct Mbuf {
    /// `m_hdr`.
    pub m_hdr: MHdr,
    /// `M_dat`: the data area, read and written through raw pointers and the views below.
    m_dat: UnsafeCell<MDat>,
}

impl Mbuf {
    /// `m_next`: next buffer in chain.
    #[inline]
    pub fn m_next(&self) -> &Cell<Option<&'static Mbuf>> {
        &self.m_hdr.mh_next
    }

    /// `m_len`: amount of data in this mbuf.
    #[inline]
    pub fn m_len(&self) -> &Cell<u32> {
        &self.m_hdr.mh_len
    }

    /// `m_data`: location of data.
    #[inline]
    pub fn m_data(&self) -> &Cell<*mut u8> {
        &self.m_hdr.mh_data
    }

    /// `m_type`: type of data in this mbuf.
    #[inline]
    pub fn m_type(&self) -> &Cell<i16> {
        &self.m_hdr.mh_type
    }

    /// `m_flags`.
    #[inline]
    pub fn m_flags(&self) -> &Cell<u16> {
        &self.m_hdr.mh_flags
    }

    /// `m_nextpkt`: next chain in queue/record.
    #[inline]
    pub fn m_nextpkt(&self) -> &Cell<Option<&'static Mbuf>> {
        &self.m_hdr.mh_nextpkt
    }

    /// `m_pkthdr`: the packet header at the start of the data area (`M_PKTHDR` set).
    #[inline]
    pub fn m_pkthdr(&self) -> &Pkthdr {
        // SAFETY: the area is 8-aligned and longer than a `Pkthdr` (`MHLEN` is positive, checked
        // below); a `Pkthdr` is `Cell`s of integers, raw pointers and a list head (a raw
        // pointer), valid for any initialised bytes, and the `Cell`s make writes through the
        // shared reference legal inside the `UnsafeCell`.
        unsafe { &*self.m_dat.get().cast::<Pkthdr>() }
    }

    /// `m_ext`: the external storage's description, after the packet header (`M_EXT` set).
    #[inline]
    pub fn m_ext(&self) -> &MbufExt {
        // SAFETY: as for `m_pkthdr`: the bytes after the packet header are 8-aligned (the
        // header's size is a multiple of 8) and hold a `MbufExt` (checked below), whose fields
        // are `Cell`s written by `mextadd` before `M_EXT` is set.
        unsafe {
            &*self
                .m_dat
                .get()
                .cast::<u8>()
                .add(size_of::<Pkthdr>())
                .cast::<MbufExt>()
        }
    }

    /// `m_pktdat`: the data area after the packet header (`MHLEN` bytes).
    #[inline]
    pub fn m_pktdat(&self) -> *mut u8 {
        self.m_dat
            .get()
            .cast::<u8>()
            .wrapping_add(size_of::<Pkthdr>())
    }

    /// `m_dat`: the whole data area (`MLEN` bytes).
    #[inline]
    pub fn m_dat(&self) -> *mut u8 {
        self.m_dat.get().cast::<u8>()
    }

    /// `to->m_pkthdr = from->m_pkthdr`: the structure assignment, a copy of the header's bytes.
    pub fn m_pkthdr_assign(&self, from: &Mbuf) {
        // SAFETY: both areas are `MLEN` bytes inside their `UnsafeCell`s and a `Pkthdr` has no
        // destructor, so a byte copy is the C's assignment; `copy` allows `from == self`.
        unsafe {
            ptr::copy(
                from.m_dat.get().cast::<u8>(),
                self.m_dat.get().cast::<u8>(),
                size_of::<Pkthdr>(),
            );
        }
    }

    /// `memset(&m->m_pkthdr, 0, sizeof(m->m_pkthdr))`.
    pub fn m_pkthdr_zero(&self) {
        // SAFETY: the header's bytes lie inside the `UnsafeCell`; zero is a valid value of every
        // field (null pointers, an empty tag list, zero counts).
        unsafe { ptr::write_bytes(self.m_dat.get().cast::<u8>(), 0, size_of::<Pkthdr>()) };
    }

    /// `memcpy(&to->m_ext, &from->m_ext, sizeof(struct mbuf_ext))`.
    pub fn m_ext_assign(&self, from: &Mbuf) {
        // SAFETY: as for `m_pkthdr_assign`, for the `MbufExt` after the packet header.
        unsafe {
            ptr::copy(
                from.m_pktdat().cast_const(),
                self.m_pktdat(),
                size_of::<MbufExt>(),
            );
        }
    }
}

/// `enum mbstat_counters`: the counters after the per-type ones in `mbstat`.
#[repr(usize)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MbstatCounters {
    /// `mbs_drops`.
    MbsDrops = MT_NTYPES,
    /// `mbs_wait`.
    MbsWait,
    /// `mbs_drain`.
    MbsDrain,
    /// `mbs_defrag_alloc`.
    MbsDefragAlloc,
    /// `mbs_prepend_alloc`.
    MbsPrependAlloc,
    /// `mbs_pullup_alloc`.
    MbsPullupAlloc,
    /// `mbs_pullup_copy`.
    MbsPullupCopy,
    /// `mbs_pulldown_alloc`.
    MbsPulldownAlloc,
    /// `mbs_pulldown_copy`.
    MbsPulldownCopy,
    /// `mbs_ncounters`.
    MbsNcounters,
}

/// `struct mbstat`: mbuf statistics. For statistics related to mbuf and cluster allocations,
/// see also the pool headers (`mbpool` and `mclpool`). `#[repr(C)]`: `kern.mbstat` copies it
/// out.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Mbstat {
    /// `m_drops`: times failed to find space.
    pub m_drops: u64,
    /// `m_wait`: times waited for space.
    pub m_wait: u64,
    /// `m_drain`: times drained protocols for space.
    pub m_drain: u64,
    /// `m_mtypes`: type specific mbuf allocations.
    pub m_mtypes: [u64; MT_NTYPES],
    /// `m_defrag_alloc`.
    pub m_defrag_alloc: u64,
    /// `m_prepend_alloc`.
    pub m_prepend_alloc: u64,
    /// `m_pullup_alloc`.
    pub m_pullup_alloc: u64,
    /// `m_pullup_copy`.
    pub m_pullup_copy: u64,
    /// `m_pulldown_alloc`.
    pub m_pulldown_alloc: u64,
    /// `m_pulldown_copy`.
    pub m_pulldown_copy: u64,
}

/// `struct mbuf_list`: packets linked through `m_nextpkt`.
pub struct MbufList {
    /// `ml_head`.
    pub ml_head: Cell<Option<&'static Mbuf>>,
    /// `ml_tail`.
    pub ml_tail: Cell<Option<&'static Mbuf>>,
    /// `ml_len`: the number of packets; atomic so `mq_len` can read it unlocked.
    pub ml_len: AtomicU32,
}

impl MbufList {
    /// `MBUF_LIST_INITIALIZER()`.
    pub const fn new() -> Self {
        Self {
            ml_head: Cell::new(None),
            ml_tail: Cell::new(None),
            ml_len: AtomicU32::new(0),
        }
    }

    /// `MBUF_LIST_FOREACH(ml, m)`.
    pub fn iter(&self) -> MbufListIter {
        MbufListIter {
            m: mbuf_list_first(self),
        }
    }
}

impl Default for MbufList {
    fn default() -> Self {
        Self::new()
    }
}

/// The iterator behind `MBUF_LIST_FOREACH`.
pub struct MbufListIter {
    m: Option<&'static Mbuf>,
}

impl Iterator for MbufListIter {
    type Item = &'static Mbuf;

    fn next(&mut self) -> Option<&'static Mbuf> {
        let m = self.m?;
        self.m = mbuf_list_next(m);
        Some(m)
    }
}

/// `struct mbuf_queue`: a locked, bounded `mbuf_list`.
pub struct MbufQueue {
    /// `mq_mtx`.
    pub mq_mtx: Mutex,
    /// `mq_list`. Protected by: `mq_mtx`.
    pub mq_list: MbufList,
    /// `mq_maxlen`. Written under `mq_mtx`, read with `READ_ONCE`.
    pub mq_maxlen: AtomicU32,
    /// `mq_drops`. Written under `mq_mtx`, read with `READ_ONCE`.
    pub mq_drops: AtomicU32,
}

// SAFETY: `mq_list`'s `Cell`s are only touched with `mq_mtx` held; the rest is atomic or the
// mutex itself.
unsafe impl Sync for MbufQueue {}

impl MbufQueue {
    /// `MBUF_QUEUE_INITIALIZER(maxlen, ipl)`.
    pub const fn new(maxlen: u32, ipl: i32) -> Self {
        Self {
            mq_mtx: Mutex::new(ipl),
            mq_list: MbufList::new(),
            mq_maxlen: AtomicU32::new(maxlen),
            mq_drops: AtomicU32::new(0),
        }
    }
}

/// `mtod(m, t)`: convert mbuf pointer to data pointer of correct type.
#[inline]
pub fn mtod<T>(m: &Mbuf) -> *mut T {
    m.m_data().get().cast::<T>()
}

/// `MCLREFDEBUGN(m, file, line)` (`DEBUG`).
#[cfg(feature = "debug")]
pub fn mclrefdebugn(m: &Mbuf, file: &'static str, line: i32) {
    m.m_ext().ext_nfile.set(ptr::from_ref(file));
    m.m_ext().ext_nline.set(line);
}

/// `MCLREFDEBUGO(m, file, line)` (`DEBUG`).
#[cfg(feature = "debug")]
pub fn mclrefdebugo(m: &Mbuf, file: &'static str, line: i32) {
    m.m_ext().ext_ofile.set(ptr::from_ref(file));
    m.m_ext().ext_oline.set(line);
}

/// `m_extreferenced`: whether the cluster is shared with another mbuf. Only meaningful with
/// `M_EXT` set (`M_READONLY` asks only then); a plain mbuf answers `false`.
#[inline]
pub fn m_extreferenced(m: &Mbuf) -> bool {
    if m.m_flags().get() & M_EXT == 0
        || m.m_ext().ext_free_fn.get() != M_EXTFREE_REFS_FN.load(Ordering::Relaxed)
    {
        return false;
    }

    // SAFETY: `M_EXT` with the refs free function: `ext_arg` is the cluster's `m_ext_refs`.
    unsafe { m_ext_refs_shared(m) }
}

/// `MCLISREFERENCED(m)`.
#[inline]
pub fn mclisreferenced(m: &Mbuf) -> bool {
    m_extreferenced(m)
}

/// `MCLINITREFERENCE(m)`: with `DEBUG`, records where the reference was made.
#[inline]
#[track_caller]
pub fn mclinitreference(m: &Mbuf) {
    #[cfg(feature = "debug")]
    {
        let loc = core::panic::Location::caller();
        mclrefdebugo(m, loc.file(), loc.line() as i32);
        mclrefdebugn(m, "", 0);
    }
    #[cfg(not(feature = "debug"))]
    let _ = m;
}

/// `MEXTADD(m, buf, size, mflags, freefn, arg)`: adds pre-allocated external storage to a
/// normal mbuf; the flag `M_EXT` is set.
#[track_caller]
pub fn mextadd(m: &Mbuf, buf: *mut u8, size: u32, mflags: u16, freefn: u32, arg: *mut ()) {
    m.m_ext().ext_buf.set(buf);
    m.m_data().set(buf);
    m.m_flags()
        .set(m.m_flags().get() | M_EXT | (mflags & M_EXTWR));
    m.m_ext().ext_size.set(size);
    m.m_ext().ext_free_fn.set(freefn);
    m.m_ext().ext_arg.set(arg);
    mclinitreference(m);
}

/// `MCLGET(m, how)`: allocates and adds an mbuf cluster to a normal mbuf; the flag `M_EXT` is
/// set upon success.
pub fn mclget(m: &'static Mbuf, how: i32) {
    let _ = m_clget(Some(m), how, MCLBYTES as u32);
}

/// `MCLGETL(m, how, l)`: as `MCLGET`, for a cluster of at least `l` bytes.
pub fn mclgetl(m: &'static Mbuf, how: i32, l: u32) -> Option<&'static Mbuf> {
    m_clget(Some(m), how, l)
}

/// `M_MOVE_HDR(to, from)`: move just `m_pkthdr` from `from` to `to`, remove `M_PKTHDR` and
/// clean flags/tags for `from`.
pub fn m_move_hdr(to: &Mbuf, from: &Mbuf) {
    to.m_pkthdr_assign(from);
    from.m_flags().set(from.m_flags().get() & !M_PKTHDR);
    from.m_pkthdr().ph_tags.init();
    from.m_pkthdr().pf.statekey.set(ptr::null_mut());
}

/// `M_MOVE_PKTHDR(to, from)`: move mbuf pkthdr from `from` to `to`. `from` must have
/// `M_PKTHDR` set, and `to` must be empty.
pub fn m_move_pkthdr(to: &Mbuf, from: &Mbuf) {
    to.m_flags().set(to.m_flags().get() & (M_EXT | M_EXTWR));
    to.m_flags()
        .set(to.m_flags().get() | (from.m_flags().get() & M_COPYFLAGS));
    m_move_hdr(to, from);
    if to.m_flags().get() & M_EXT == 0 {
        to.m_data().set(to.m_pktdat());
    }
}

/// `M_READONLY(m)`: determine if an mbuf's data area is read-only. This is true for
/// non-cluster external storage and for clusters that are being referenced by more than one
/// mbuf.
#[inline]
pub fn m_readonly(m: &Mbuf) -> bool {
    m.m_flags().get() & M_EXT != 0 && (m.m_flags().get() & M_EXTWR == 0 || mclisreferenced(m))
}

/// `m_freemp`: frees the packet `*mp` and clears the caller's pointer.
pub fn m_freemp(mp: &mut Option<&'static Mbuf>) -> Option<&'static Mbuf> {
    let m = mp.take();
    m_freem(m)
}

/// `mbstat_inc`: bumps counter `c` (an `MT_*` type or an [`MbstatCounters`] value) at `splnet`.
#[inline]
pub fn mbstat_inc(c: usize) {
    let s = splnet();
    counters_inc(mbstat(), c);
    splx(s);
}

/// `ml_len(ml)`.
#[inline]
pub fn ml_len(ml: &MbufList) -> u32 {
    ml.ml_len.load(Ordering::Relaxed)
}

/// `ml_empty(ml)`.
#[inline]
pub fn ml_empty(ml: &MbufList) -> bool {
    ml_len(ml) == 0
}

/// `MBUF_LIST_FIRST(ml)`.
#[inline]
pub fn mbuf_list_first(ml: &MbufList) -> Option<&'static Mbuf> {
    ml.ml_head.get()
}

/// `MBUF_LIST_NEXT(m)`.
#[inline]
pub fn mbuf_list_next(m: &Mbuf) -> Option<&'static Mbuf> {
    m.m_nextpkt().get()
}

/// `mq_len(mq)`.
#[inline]
pub fn mq_len(mq: &MbufQueue) -> u32 {
    mq.mq_list.ml_len.load(Ordering::Relaxed)
}

/// `mq_empty(mq)`.
#[inline]
pub fn mq_empty(mq: &MbufQueue) -> bool {
    mq_len(mq) == 0
}

/// `mq_full(mq)`.
#[inline]
pub fn mq_full(mq: &MbufQueue) -> bool {
    mq_len(mq) >= mq.mq_maxlen.load(Ordering::Relaxed)
}

/// `mq_drops(mq)`.
#[inline]
pub fn mq_drops(mq: &MbufQueue) -> u32 {
    mq.mq_drops.load(Ordering::Relaxed)
}

// CTASSERT(MSIZE == sizeof(struct mbuf)) is mbinit's; the views must fit the data area.
const _: () = {
    assert!(MSIZE == size_of::<Mbuf>());
    assert!(size_of::<Pkthdr>().is_multiple_of(8));
    assert!(size_of::<MbufExt>() <= MHLEN);
    assert!(MHLEN > 0);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `<sys/mbuf.h>`: the layout and, against the C header, the constants.

    use super::*;

    #[test]
    fn layout_matches_lp64_openbsd() {
        assert_eq!(size_of::<MHdr>(), 32);
        assert_eq!(MLEN, 224);
        assert_eq!(size_of::<Pkthdr>(), 80);
        assert_eq!(MHLEN, 144);
        assert_eq!(MINCLSIZE, 369);
        assert_eq!(size_of::<MTag>(), 16);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/mbuf.h");
        let ours: &[(&str, i64)] = &[
            ("MSIZE", MSIZE as i64),
            ("MAXMCLBYTES", MAXMCLBYTES as i64),
            ("MCLSHIFT", MCLSHIFT as i64),
            ("M_EXT", i64::from(M_EXT)),
            ("M_PKTHDR", i64::from(M_PKTHDR)),
            ("M_EOR", i64::from(M_EOR)),
            ("M_EXTWR", i64::from(M_EXTWR)),
            ("M_PROTO1", i64::from(M_PROTO1)),
            ("M_VLANTAG", i64::from(M_VLANTAG)),
            ("M_LOOP", i64::from(M_LOOP)),
            ("M_BCAST", i64::from(M_BCAST)),
            ("M_MCAST", i64::from(M_MCAST)),
            ("M_CONF", i64::from(M_CONF)),
            ("M_AUTH", i64::from(M_AUTH)),
            ("M_TUNNEL", i64::from(M_TUNNEL)),
            ("M_ZEROIZE", i64::from(M_ZEROIZE)),
            ("M_COMP", i64::from(M_COMP)),
            ("M_LINK0", i64::from(M_LINK0)),
            ("M_IPV4_CSUM_OUT", i64::from(M_IPV4_CSUM_OUT)),
            ("M_TCP_CSUM_OUT", i64::from(M_TCP_CSUM_OUT)),
            ("M_UDP_CSUM_OUT", i64::from(M_UDP_CSUM_OUT)),
            ("M_IPV4_CSUM_IN_OK", i64::from(M_IPV4_CSUM_IN_OK)),
            ("M_IPV4_CSUM_IN_BAD", i64::from(M_IPV4_CSUM_IN_BAD)),
            ("M_TCP_CSUM_IN_OK", i64::from(M_TCP_CSUM_IN_OK)),
            ("M_TCP_CSUM_IN_BAD", i64::from(M_TCP_CSUM_IN_BAD)),
            ("M_UDP_CSUM_IN_OK", i64::from(M_UDP_CSUM_IN_OK)),
            ("M_UDP_CSUM_IN_BAD", i64::from(M_UDP_CSUM_IN_BAD)),
            ("M_ICMP_CSUM_OUT", i64::from(M_ICMP_CSUM_OUT)),
            ("M_ICMP_CSUM_IN_OK", i64::from(M_ICMP_CSUM_IN_OK)),
            ("M_ICMP_CSUM_IN_BAD", i64::from(M_ICMP_CSUM_IN_BAD)),
            ("M_IPV6_DF_OUT", i64::from(M_IPV6_DF_OUT)),
            ("M_TIMESTAMP", i64::from(M_TIMESTAMP)),
            ("M_FLOWID", i64::from(M_FLOWID)),
            ("M_TCP_TSO", i64::from(M_TCP_TSO)),
            ("PF_TAG_GENERATED", i64::from(PF_TAG_GENERATED)),
            (
                "PF_TAG_SYNCOOKIE_RECREATED",
                i64::from(PF_TAG_SYNCOOKIE_RECREATED),
            ),
            (
                "PF_TAG_TRANSLATE_LOCALHOST",
                i64::from(PF_TAG_TRANSLATE_LOCALHOST),
            ),
            ("PF_TAG_DIVERTED", i64::from(PF_TAG_DIVERTED)),
            ("PF_TAG_DIVERTED_PACKET", i64::from(PF_TAG_DIVERTED_PACKET)),
            ("PF_TAG_REROUTE", i64::from(PF_TAG_REROUTE)),
            ("PF_TAG_REFRAGMENTED", i64::from(PF_TAG_REFRAGMENTED)),
            ("PF_TAG_PROCESSED", i64::from(PF_TAG_PROCESSED)),
            ("MT_FREE", i64::from(MT_FREE)),
            ("MT_DATA", i64::from(MT_DATA)),
            ("MT_HEADER", i64::from(MT_HEADER)),
            ("MT_SONAME", i64::from(MT_SONAME)),
            ("MT_SOOPTS", i64::from(MT_SOOPTS)),
            ("MT_FTABLE", i64::from(MT_FTABLE)),
            ("MT_CONTROL", i64::from(MT_CONTROL)),
            ("MT_OOBDATA", i64::from(MT_OOBDATA)),
            ("MT_NTYPES", MT_NTYPES as i64),
            ("MEXTFREE_POOL", i64::from(MEXTFREE_POOL)),
            ("M_COPYALL", i64::from(M_COPYALL)),
            (
                "PACKET_TAG_IPSEC_IN_DONE",
                i64::from(PACKET_TAG_IPSEC_IN_DONE),
            ),
            (
                "PACKET_TAG_IPSEC_OUT_DONE",
                i64::from(PACKET_TAG_IPSEC_OUT_DONE),
            ),
            (
                "PACKET_TAG_IPSEC_FLOWINFO",
                i64::from(PACKET_TAG_IPSEC_FLOWINFO),
            ),
            ("PACKET_TAG_IP_OFFNXT", i64::from(PACKET_TAG_IP_OFFNXT)),
            ("PACKET_TAG_IP6_OFFNXT", i64::from(PACKET_TAG_IP6_OFFNXT)),
            ("PACKET_TAG_WIREGUARD", i64::from(PACKET_TAG_WIREGUARD)),
            ("PACKET_TAG_GRE", i64::from(PACKET_TAG_GRE)),
            ("PACKET_TAG_DLT", i64::from(PACKET_TAG_DLT)),
            ("PACKET_TAG_PF_DIVERT", i64::from(PACKET_TAG_PF_DIVERT)),
            (
                "PACKET_TAG_PF_REASSEMBLED",
                i64::from(PACKET_TAG_PF_REASSEMBLED),
            ),
            ("PACKET_TAG_SRCROUTE", i64::from(PACKET_TAG_SRCROUTE)),
            ("PACKET_TAG_TUNNEL", i64::from(PACKET_TAG_TUNNEL)),
            ("PACKET_TAG_CARP_BAL_IP", i64::from(PACKET_TAG_CARP_BAL_IP)),
            ("PACKET_TAG_MAXSIZE", PACKET_TAG_MAXSIZE as i64),
            ("M_MAXLOOP", i64::from(M_MAXLOOP)),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
