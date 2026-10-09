/* <LICENSES> */
/*
 * Copyright (c) 1982, 1985, 1986, 1988, 1993, 1994
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
 *	@(#)socket.h	8.4 (Berkeley) 2/21/94
 */
/* </LICENSES> */

/* <CODE> */
//! Sockets: types, address families, options and the generic socket address,
//! `<sys/socket.h>`.
//!
//! Upstream: sys/sys/socket.h @ 3ce1f3f79392
//!
//! The structures cross the user/kernel boundary, so they are `#[repr(C)]` with the C's field
//! order, and their LP64 sizes are checked at compile time. Address families are
//! [`SaFamily`] (`sa_family_t`, the type of `sa_family`), which is what the network code
//! compares them with; the other constants are `i32`, the `int` the system calls take.
//!
//! Status: `wip`.
//!
//! ## Deviations
//! - `msg_iov` is a `*mut c_void` until `<sys/uio.h>` (`struct iovec`) is ported.
//! - `CTL_NET_NAMES`, `CTL_NET_RT_NAMES`, `CTL_NET_UNIX_NAMES`, `CTL_NET_UNIX_PROTO_NAMES`,
//!   `CTL_NET_LINK_NAMES`, `CTL_NET_LINK_IFRXQ_NAMES`, `CTL_NET_KEY_NAMES`,
//!   `CTL_NET_BPF_NAMES` and `CTL_NET_PFLOW_NAMES` (`struct ctlname` tables) come with
//!   `<sys/sysctl.h>`.
//! - `CMSG_DATA`, `CMSG_FIRSTHDR` and `CMSG_NXTHDR` work on raw pointers, as the C does on
//!   `msg_control`: the first two only compute addresses and are safe; [`cmsg_nxthdr`] reads
//!   `cmsg_len` and is `unsafe`. `CMSG_ALIGN`, `CMSG_LEN` and `CMSG_SPACE` are `const fn`s.
//! - `sstosa` is a pointer cast (see `docs/C_TO_RUST.md`); `SA_LEN(x)` is `x.sa_len`.
//! - The userland prototypes (`socket`, `bind`, ...) have no kernel counterpart; the system
//!   calls come with `uipc_syscalls.c`.

use core::ffi::c_void;
use core::mem::size_of;

use crate::sys::param::align;
use crate::sys::time::Timeval;
use crate::sys::types::{Gid, Off, Pid, SaFamily, Socklen, Uid};

// Types

/// Stream socket.
pub const SOCK_STREAM: i32 = 1;
/// Datagram socket.
pub const SOCK_DGRAM: i32 = 2;
/// Raw-protocol interface.
pub const SOCK_RAW: i32 = 3;
/// Reliably-delivered message.
pub const SOCK_RDM: i32 = 4;
/// Sequenced packet stream.
pub const SOCK_SEQPACKET: i32 = 5;
/// Mask that covers the above.
pub const SOCK_TYPE_MASK: i32 = 0x000F;

// Socket creation flags

/// Set `FD_CLOEXEC`.
pub const SOCK_CLOEXEC: i32 = 0x8000;
/// Set `O_NONBLOCK`.
pub const SOCK_NONBLOCK: i32 = 0x4000;
/// Inherit `O_NONBLOCK` from listener.
pub const SOCK_NONBLOCK_INHERIT: i32 = 0x2000;
/// Set `SS_DNS`.
pub const SOCK_DNS: i32 = 0x1000;
/// Set `FD_CLOFORK`.
pub const SOCK_CLOFORK: i32 = 0x0800;

// Option flags per-socket.

/// Turn on debugging info recording.
pub const SO_DEBUG: i32 = 0x0001;
/// Socket has had `listen()`.
pub const SO_ACCEPTCONN: i32 = 0x0002;
/// Allow local address reuse.
pub const SO_REUSEADDR: i32 = 0x0004;
/// Keep connections alive.
pub const SO_KEEPALIVE: i32 = 0x0008;
/// Just use interface addresses.
pub const SO_DONTROUTE: i32 = 0x0010;
/// Permit sending of broadcast msgs.
pub const SO_BROADCAST: i32 = 0x0020;
/// Bypass hardware when possible.
pub const SO_USELOOPBACK: i32 = 0x0040;
/// Linger on close if data present.
pub const SO_LINGER: i32 = 0x0080;
/// Leave received OOB data in line.
pub const SO_OOBINLINE: i32 = 0x0100;
/// Allow local address & port reuse.
pub const SO_REUSEPORT: i32 = 0x0200;
/// Timestamp received dgram traffic.
pub const SO_TIMESTAMP: i32 = 0x0800;
/// Allow bind to any address.
pub const SO_BINDANY: i32 = 0x1000;
/// Zero out all mbufs sent over socket.
pub const SO_ZEROIZE: i32 = 0x2000;

// Additional options, not kept in so_options.

/// Send buffer size.
pub const SO_SNDBUF: i32 = 0x1001;
/// Receive buffer size.
pub const SO_RCVBUF: i32 = 0x1002;
/// Send low-water mark.
pub const SO_SNDLOWAT: i32 = 0x1003;
/// Receive low-water mark.
pub const SO_RCVLOWAT: i32 = 0x1004;
/// Send timeout.
pub const SO_SNDTIMEO: i32 = 0x1005;
/// Receive timeout.
pub const SO_RCVTIMEO: i32 = 0x1006;
/// Get error status and clear.
pub const SO_ERROR: i32 = 0x1007;
/// Get socket type.
pub const SO_TYPE: i32 = 0x1008;
/// Routing table to be used.
pub const SO_RTABLE: i32 = 0x1021;
/// Get connect-time credentials.
pub const SO_PEERCRED: i32 = 0x1022;
/// Splice data to other socket.
pub const SO_SPLICE: i32 = 0x1023;
/// Get socket domain.
pub const SO_DOMAIN: i32 = 0x1024;
/// Get socket protocol.
pub const SO_PROTOCOL: i32 = 0x1025;

// Maximum number of alternate routing tables

/// Highest routing table id.
pub const RT_TABLEID_MAX: u32 = 255;
/// Bits of a routing table id.
pub const RT_TABLEID_BITS: u32 = 8;
/// Mask of a routing table id.
pub const RT_TABLEID_MASK: u32 = 0xff;

/// Level number for (get/set)sockopt() to apply to socket itself: options for socket level.
pub const SOL_SOCKET: i32 = 0xffff;

// Address families.

/// Unspecified.
pub const AF_UNSPEC: SaFamily = 0;
/// Local to host.
pub const AF_UNIX: SaFamily = 1;
/// Draft POSIX compatibility.
pub const AF_LOCAL: SaFamily = AF_UNIX;
/// Internetwork: UDP, TCP, etc.
pub const AF_INET: SaFamily = 2;
/// Arpanet imp addresses.
pub const AF_IMPLINK: SaFamily = 3;
/// Pup protocols: e.g. BSP.
pub const AF_PUP: SaFamily = 4;
/// MIT CHAOS protocols.
pub const AF_CHAOS: SaFamily = 5;
/// XEROX NS protocols.
pub const AF_NS: SaFamily = 6;
/// ISO protocols.
pub const AF_ISO: SaFamily = 7;
/// ISO protocols.
pub const AF_OSI: SaFamily = AF_ISO;
/// European computer manufacturers.
pub const AF_ECMA: SaFamily = 8;
/// Datakit protocols.
pub const AF_DATAKIT: SaFamily = 9;
/// CCITT protocols, X.25 etc.
pub const AF_CCITT: SaFamily = 10;
/// IBM SNA.
pub const AF_SNA: SaFamily = 11;
/// DECnet.
#[allow(non_upper_case_globals)] // the C name
pub const AF_DECnet: SaFamily = 12;
/// DEC Direct data link interface.
pub const AF_DLI: SaFamily = 13;
/// LAT.
pub const AF_LAT: SaFamily = 14;
/// NSC Hyperchannel.
pub const AF_HYLINK: SaFamily = 15;
/// Apple Talk.
pub const AF_APPLETALK: SaFamily = 16;
/// Internal Routing Protocol.
pub const AF_ROUTE: SaFamily = 17;
/// Link layer interface.
pub const AF_LINK: SaFamily = 18;
/// eXpress Transfer Protocol (no AF).
#[allow(non_upper_case_globals)] // the C name
pub const pseudo_AF_XTP: SaFamily = 19;
/// Connection-oriented IP, aka ST II.
pub const AF_COIP: SaFamily = 20;
/// Computer Network Technology.
pub const AF_CNT: SaFamily = 21;
/// Help Identify RTIP packets.
#[allow(non_upper_case_globals)] // the C name
pub const pseudo_AF_RTIP: SaFamily = 22;
/// Novell Internet Protocol.
pub const AF_IPX: SaFamily = 23;
/// IPv6.
pub const AF_INET6: SaFamily = 24;
/// Help Identify PIP packets.
#[allow(non_upper_case_globals)] // the C name
pub const pseudo_AF_PIP: SaFamily = 25;
/// Integrated Services Digital Network.
pub const AF_ISDN: SaFamily = 26;
/// CCITT E.164 recommendation.
pub const AF_E164: SaFamily = AF_ISDN;
/// Native ATM access.
pub const AF_NATM: SaFamily = 27;
/// `AF_ENCAP`.
pub const AF_ENCAP: SaFamily = 28;
/// Simple Internet Protocol.
pub const AF_SIP: SaFamily = 29;
/// `AF_KEY`.
pub const AF_KEY: SaFamily = 30;
/// Used by BPF to not rewrite headers in interface output routine.
#[allow(non_upper_case_globals)] // the C name
pub const pseudo_AF_HDRCMPLT: SaFamily = 31;
/// Bluetooth.
pub const AF_BLUETOOTH: SaFamily = 32;
/// MPLS.
pub const AF_MPLS: SaFamily = 33;
/// pflow.
#[allow(non_upper_case_globals)] // the C name
pub const pseudo_AF_PFLOW: SaFamily = 34;
/// PIPEX.
#[allow(non_upper_case_globals)] // the C name
pub const pseudo_AF_PIPEX: SaFamily = 35;
/// Frame (Ethernet) sockets.
pub const AF_FRAME: SaFamily = 36;
/// One past the highest address family.
pub const AF_MAX: SaFamily = 37;

// Protocol families, same as address families for now.

/// `PF_UNSPEC`.
pub const PF_UNSPEC: SaFamily = AF_UNSPEC;
/// `PF_LOCAL`.
pub const PF_LOCAL: SaFamily = AF_LOCAL;
/// `PF_UNIX`.
pub const PF_UNIX: SaFamily = AF_UNIX;
/// `PF_INET`.
pub const PF_INET: SaFamily = AF_INET;
/// `PF_IMPLINK`.
pub const PF_IMPLINK: SaFamily = AF_IMPLINK;
/// `PF_PUP`.
pub const PF_PUP: SaFamily = AF_PUP;
/// `PF_CHAOS`.
pub const PF_CHAOS: SaFamily = AF_CHAOS;
/// `PF_NS`.
pub const PF_NS: SaFamily = AF_NS;
/// `PF_ISO`.
pub const PF_ISO: SaFamily = AF_ISO;
/// `PF_OSI`.
pub const PF_OSI: SaFamily = AF_ISO;
/// `PF_ECMA`.
pub const PF_ECMA: SaFamily = AF_ECMA;
/// `PF_DATAKIT`.
pub const PF_DATAKIT: SaFamily = AF_DATAKIT;
/// `PF_CCITT`.
pub const PF_CCITT: SaFamily = AF_CCITT;
/// `PF_SNA`.
pub const PF_SNA: SaFamily = AF_SNA;
/// `PF_DECnet`.
#[allow(non_upper_case_globals)] // the C name
pub const PF_DECnet: SaFamily = AF_DECnet;
/// `PF_DLI`.
pub const PF_DLI: SaFamily = AF_DLI;
/// `PF_LAT`.
pub const PF_LAT: SaFamily = AF_LAT;
/// `PF_HYLINK`.
pub const PF_HYLINK: SaFamily = AF_HYLINK;
/// `PF_APPLETALK`.
pub const PF_APPLETALK: SaFamily = AF_APPLETALK;
/// `PF_ROUTE`.
pub const PF_ROUTE: SaFamily = AF_ROUTE;
/// `PF_LINK`.
pub const PF_LINK: SaFamily = AF_LINK;
/// Really just proto family, no AF.
pub const PF_XTP: SaFamily = pseudo_AF_XTP;
/// `PF_COIP`.
pub const PF_COIP: SaFamily = AF_COIP;
/// `PF_CNT`.
pub const PF_CNT: SaFamily = AF_CNT;
/// Same format as `AF_NS`.
pub const PF_IPX: SaFamily = AF_IPX;
/// `PF_INET6`.
pub const PF_INET6: SaFamily = AF_INET6;
/// Same format as `AF_INET`.
pub const PF_RTIP: SaFamily = pseudo_AF_RTIP;
/// `PF_PIP`.
pub const PF_PIP: SaFamily = pseudo_AF_PIP;
/// `PF_ISDN`.
pub const PF_ISDN: SaFamily = AF_ISDN;
/// `PF_NATM`.
pub const PF_NATM: SaFamily = AF_NATM;
/// `PF_ENCAP`.
pub const PF_ENCAP: SaFamily = AF_ENCAP;
/// `PF_SIP`.
pub const PF_SIP: SaFamily = AF_SIP;
/// `PF_KEY`.
pub const PF_KEY: SaFamily = AF_KEY;
/// `PF_BPF`.
pub const PF_BPF: SaFamily = pseudo_AF_HDRCMPLT;
/// `PF_BLUETOOTH`.
pub const PF_BLUETOOTH: SaFamily = AF_BLUETOOTH;
/// `PF_MPLS`.
pub const PF_MPLS: SaFamily = AF_MPLS;
/// `PF_PFLOW`.
pub const PF_PFLOW: SaFamily = pseudo_AF_PFLOW;
/// `PF_PIPEX`.
pub const PF_PIPEX: SaFamily = pseudo_AF_PIPEX;
/// `PF_FRAME`.
pub const PF_FRAME: SaFamily = AF_FRAME;
/// `PF_MAX`.
pub const PF_MAX: SaFamily = AF_MAX;

// The valid values for the "how" field used by shutdown(2).

/// Shut down the receive side.
pub const SHUT_RD: i32 = 0;
/// Shut down the send side.
pub const SHUT_WR: i32 = 1;
/// Shut down both sides.
pub const SHUT_RDWR: i32 = 2;

// Definitions for network related sysctl, CTL_NET. Second level is protocol family, third
// level is protocol number; further levels are defined by the individual families below.

/// `NET_MAXID`.
pub const NET_MAXID: i32 = AF_MAX as i32;

// PF_ROUTE - Routing table. Four additional levels: address family (0 is wildcard), type of
// info (below), flag(s) to mask with for NET_RT_FLAGS, routing table to use (facultative,
// defaults to 0; NET_RT_TABLE has the table id as sixth element).

/// Dump; may limit to a.f.
pub const NET_RT_DUMP: i32 = 1;
/// By flags, e.g. RESOLVING.
pub const NET_RT_FLAGS: i32 = 2;
/// Survey interface list.
pub const NET_RT_IFLIST: i32 = 3;
/// Routing table statistics.
pub const NET_RT_STATS: i32 = 4;
/// `NET_RT_TABLE`.
pub const NET_RT_TABLE: i32 = 5;
/// `NET_RT_IFNAMES`.
pub const NET_RT_IFNAMES: i32 = 6;
/// `NET_RT_SOURCE`.
pub const NET_RT_SOURCE: i32 = 7;
/// `NET_RT_MAXID`.
pub const NET_RT_MAXID: i32 = 8;

// PF_UNIX - unix socket tunables

/// `NET_UNIX_INFLIGHT`.
pub const NET_UNIX_INFLIGHT: i32 = 6;
/// `NET_UNIX_DEFERRED`.
pub const NET_UNIX_DEFERRED: i32 = 7;
/// `NET_UNIX_MAXID`.
pub const NET_UNIX_MAXID: i32 = 8;

/// `UNPCTL_RECVSPACE`.
pub const UNPCTL_RECVSPACE: i32 = 1;
/// `UNPCTL_SENDSPACE`.
pub const UNPCTL_SENDSPACE: i32 = 2;
/// `NET_UNIX_PROTO_MAXID`.
pub const NET_UNIX_PROTO_MAXID: i32 = 3;

// PF_LINK - link layer or device tunables

/// net.link.ifrxq.
pub const NET_LINK_IFRXQ: i32 = 1;
/// `NET_LINK_MAXID`.
pub const NET_LINK_MAXID: i32 = 2;

/// net.link.ifrxq.pressure_return.
pub const NET_LINK_IFRXQ_PRESSURE_RETURN: i32 = 1;
/// net.link.ifrxq.pressure_drop.
pub const NET_LINK_IFRXQ_PRESSURE_DROP: i32 = 2;
/// `NET_LINK_IFRXQ_MAXID`.
pub const NET_LINK_IFRXQ_MAXID: i32 = 3;

// PF_KEY - Key Management

/// Return SADB.
pub const NET_KEY_SADB_DUMP: i32 = 1;
/// Return SPD.
pub const NET_KEY_SPD_DUMP: i32 = 2;
/// `NET_KEY_MAXID`.
pub const NET_KEY_MAXID: i32 = 3;

// PF_BPF not really a family, but connected under CTL_NET

/// Default buffer size.
pub const NET_BPF_BUFSIZE: i32 = 1;
/// Maximum buffer size.
pub const NET_BPF_MAXBUFSIZE: i32 = 2;
/// `NET_BPF_MAXID`.
pub const NET_BPF_MAXID: i32 = 3;

// PF_PFLOW not really a family, but connected under CTL_NET

/// Statistics.
pub const NET_PFLOW_STATS: i32 = 1;
/// `NET_PFLOW_MAXID`.
pub const NET_PFLOW_MAXID: i32 = 2;

/// Maximum queue length specifiable by listen(2).
pub const SOMAXCONN: i32 = 128;

/// Process out-of-band data.
pub const MSG_OOB: i32 = 0x1;
/// Peek at incoming message.
pub const MSG_PEEK: i32 = 0x2;
/// Send without using routing tables.
pub const MSG_DONTROUTE: i32 = 0x4;
/// Data completes record.
pub const MSG_EOR: i32 = 0x8;
/// Data discarded before delivery.
pub const MSG_TRUNC: i32 = 0x10;
/// Control data lost before delivery.
pub const MSG_CTRUNC: i32 = 0x20;
/// Wait for full request or error.
pub const MSG_WAITALL: i32 = 0x40;
/// This message should be nonblocking.
pub const MSG_DONTWAIT: i32 = 0x80;
/// This message rec'd as broadcast.
pub const MSG_BCAST: i32 = 0x100;
/// This message rec'd as multicast.
pub const MSG_MCAST: i32 = 0x200;
/// Do not send SIGPIPE.
pub const MSG_NOSIGNAL: i32 = 0x400;
/// Set `FD_CLOEXEC` on received fds.
pub const MSG_CMSG_CLOEXEC: i32 = 0x800;
/// Nonblocking but wait for one msg.
pub const MSG_WAITFORONE: i32 = 0x1000;
/// Set `FD_CLOFORK` on received fds.
pub const MSG_CMSG_CLOFORK: i32 = 0x2000;

// "Socket"-level control message types:

/// Access rights (array of int).
pub const SCM_RIGHTS: i32 = 0x01;
/// Timestamp (struct timeval).
pub const SCM_TIMESTAMP: i32 = 0x04;

/// `struct linger`: structure used for manipulating linger option.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Linger {
    /// Option on/off.
    pub l_onoff: i32,
    /// Linger time.
    pub l_linger: i32,
}

/// `struct splice`: structure used for manipulating splice option.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Splice {
    /// Drain socket file descriptor.
    pub sp_fd: i32,
    /// If set, maximum bytes to splice.
    pub sp_max: Off,
    /// Idle timeout.
    pub sp_idle: Timeval,
}

/// `struct sockaddr`: structure used by kernel to store most addresses.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sockaddr {
    /// Total length.
    pub sa_len: u8,
    /// Address family.
    pub sa_family: SaFamily,
    /// Actually longer; address value.
    pub sa_data: [u8; 14],
}

/// `struct sockaddr_storage`: sockaddr type which can hold any sockaddr type available in the
/// system. RFC 2553 calls the first field `__ss_len`; OpenBSD picked `ss_len`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SockaddrStorage {
    /// Total length.
    pub ss_len: u8,
    /// Address family.
    pub ss_family: SaFamily,
    /// Align to quad.
    pub __ss_pad1: [u8; 6],
    /// Force alignment for stupid compilers.
    pub __ss_pad2: u64,
    /// Pad to a total of 256 bytes.
    pub __ss_pad3: [u8; 240],
}

impl SockaddrStorage {
    /// An all-zero `sockaddr_storage` (`AF_UNSPEC`, length 0).
    pub const fn zeroed() -> Self {
        Self {
            ss_len: 0,
            ss_family: AF_UNSPEC,
            __ss_pad1: [0; 6],
            __ss_pad2: 0,
            __ss_pad3: [0; 240],
        }
    }
}

impl Default for SockaddrStorage {
    fn default() -> Self {
        Self::zeroed()
    }
}

/// `struct sockproto`: structure used by kernel to pass protocol information in raw sockets.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sockproto {
    /// Address family.
    pub sp_family: u16,
    /// Protocol.
    pub sp_protocol: u16,
}

/// `struct sockpeercred`: read using getsockopt() with `SOL_SOCKET`, `SO_PEERCRED`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sockpeercred {
    /// Effective user id.
    pub uid: Uid,
    /// Effective group id.
    pub gid: Gid,
    /// Process id.
    pub pid: Pid,
}

/// `struct msghdr`: message header for recvmsg and sendmsg calls. Used value-result for
/// recvmsg, value only for sendmsg.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Msghdr {
    /// Optional address.
    pub msg_name: *mut c_void,
    /// Size of address.
    pub msg_namelen: Socklen,
    /// Scatter/gather array (`struct iovec *`).
    pub msg_iov: *mut c_void,
    /// # elements in `msg_iov`.
    pub msg_iovlen: u32,
    /// Ancillary data, see below.
    pub msg_control: *mut c_void,
    /// Ancillary data buffer len.
    pub msg_controllen: Socklen,
    /// Flags on received message.
    pub msg_flags: i32,
}

/// `struct mmsghdr`: one message of `recvmmsg`/`sendmmsg`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Mmsghdr {
    /// The message.
    pub msg_hdr: Msghdr,
    /// Bytes transferred.
    pub msg_len: u32,
}

/// `struct cmsghdr`: header for ancillary data objects in `msg_control` buffer. Used for
/// additional information with/about a datagram not expressible by flags. The format is a
/// sequence of message elements headed by cmsghdr structures, each followed by its data.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cmsghdr {
    /// Data byte count, including hdr.
    pub cmsg_len: Socklen,
    /// Originating protocol.
    pub cmsg_level: i32,
    /// Protocol-specific type.
    pub cmsg_type: i32,
}

/// `SA_LEN(x)`: the total length of a socket address.
pub const fn sa_len(sa: &Sockaddr) -> u8 {
    sa.sa_len
}

/// `CMSG_ALIGN(n)`: round len up to next alignment boundary.
pub const fn cmsg_align(n: usize) -> usize {
    align(n)
}

/// `CMSG_DATA(cmsg)`: given pointer to struct cmsghdr, return pointer to data. Only the address
/// is computed.
pub const fn cmsg_data(cmsg: *mut Cmsghdr) -> *mut u8 {
    cmsg.cast::<u8>()
        .wrapping_add(cmsg_align(size_of::<Cmsghdr>()))
}

/// `CMSG_NXTHDR(mhdr, cmsg)`: given pointer to struct cmsghdr, return pointer to next cmsghdr,
/// or null when the next header would not fit in `msg_control`.
///
/// # Safety
///
/// `cmsg` must point to a readable `struct cmsghdr` inside `mhdr.msg_control`.
pub unsafe fn cmsg_nxthdr(mhdr: &Msghdr, cmsg: *mut Cmsghdr) -> *mut Cmsghdr {
    // SAFETY: the caller guarantees `cmsg` points to a readable header.
    let len = unsafe { (*cmsg).cmsg_len } as usize;
    let next = cmsg.cast::<u8>().wrapping_add(cmsg_align(len));
    let end = mhdr
        .msg_control
        .cast::<u8>()
        .wrapping_add(mhdr.msg_controllen as usize);
    if next.wrapping_add(cmsg_align(size_of::<Cmsghdr>())) > end {
        core::ptr::null_mut()
    } else {
        next.cast()
    }
}

/// `CMSG_FIRSTHDR(mhdr)`: the first control message, or null if `msg_controllen` is too short
/// for one (RFC 2292 requires the check, in case the kernel returns an empty list).
pub fn cmsg_firsthdr(mhdr: &Msghdr) -> *mut Cmsghdr {
    if mhdr.msg_controllen as usize >= size_of::<Cmsghdr>() {
        mhdr.msg_control.cast()
    } else {
        core::ptr::null_mut()
    }
}

/// `CMSG_LEN(len)`: length of the contents of a control message of length len.
pub const fn cmsg_len(len: usize) -> usize {
    cmsg_align(size_of::<Cmsghdr>()) + len
}

/// `CMSG_SPACE(len)`: length of the space taken up by a padded control message of length len.
pub const fn cmsg_space(len: usize) -> usize {
    cmsg_align(size_of::<Cmsghdr>()) + cmsg_align(len)
}

/// `sstosa(ss)`: a `sockaddr_storage` seen as a generic `sockaddr`.
pub const fn sstosa(ss: *mut SockaddrStorage) -> *mut Sockaddr {
    ss.cast()
}

// LP64 sizes of the C structures.
const _: () = {
    assert!(size_of::<Linger>() == 8);
    assert!(size_of::<Splice>() == 32);
    assert!(size_of::<Sockaddr>() == 16);
    assert!(size_of::<SockaddrStorage>() == 256);
    assert!(core::mem::align_of::<SockaddrStorage>() == 8);
    assert!(size_of::<Sockproto>() == 4);
    assert!(size_of::<Sockpeercred>() == 12);
    assert!(size_of::<Msghdr>() == 48);
    assert!(size_of::<Mmsghdr>() == 56);
    assert!(size_of::<Cmsghdr>() == 12);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reftest::{assert_complete, assert_defines};

    #[test]
    fn cmsg_macros() {
        assert_eq!(cmsg_len(4), 16 + 4);
        assert_eq!(cmsg_space(4), 16 + 8);
        assert_eq!(cmsg_space(0), 16);

        // Two control messages of 4 data bytes each in a 48-byte buffer.
        let mut buf = [0u64; 6];
        let control = buf.as_mut_ptr().cast::<c_void>();
        let mhdr = Msghdr {
            msg_name: core::ptr::null_mut(),
            msg_namelen: 0,
            msg_iov: core::ptr::null_mut(),
            msg_iovlen: 0,
            msg_control: control,
            msg_controllen: (2 * cmsg_space(4)) as Socklen,
            msg_flags: 0,
        };
        let first = cmsg_firsthdr(&mhdr);
        assert_eq!(first.cast::<c_void>(), control);
        assert_eq!(cmsg_data(first) as usize - first as usize, 16);
        // SAFETY: `first` points into `buf`, which is aligned for a `Cmsghdr`.
        unsafe { (*first).cmsg_len = cmsg_len(4) as Socklen };
        // SAFETY: as above.
        let second = unsafe { cmsg_nxthdr(&mhdr, first) };
        assert_eq!(second as usize - first as usize, cmsg_space(4));
        // SAFETY: `second` is the second header inside `buf`.
        unsafe { (*second).cmsg_len = cmsg_len(4) as Socklen };
        // SAFETY: as above.
        assert!(unsafe { cmsg_nxthdr(&mhdr, second) }.is_null());

        let short = Msghdr {
            msg_controllen: 4,
            ..mhdr
        };
        assert!(cmsg_firsthdr(&short).is_null());
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/socket.h");
        let sock = assert_defines!(defs;
        SOCK_STREAM, SOCK_DGRAM, SOCK_RAW, SOCK_RDM, SOCK_SEQPACKET, SOCK_TYPE_MASK,
        SOCK_CLOEXEC, SOCK_NONBLOCK, SOCK_NONBLOCK_INHERIT, SOCK_DNS, SOCK_CLOFORK);
        assert_complete(&defs, "SOCK_", &sock);
        let so = assert_defines!(defs;
        SO_DEBUG, SO_ACCEPTCONN, SO_REUSEADDR, SO_KEEPALIVE, SO_DONTROUTE, SO_BROADCAST,
        SO_USELOOPBACK, SO_LINGER, SO_OOBINLINE, SO_REUSEPORT, SO_TIMESTAMP, SO_BINDANY,
        SO_ZEROIZE, SO_SNDBUF, SO_RCVBUF, SO_SNDLOWAT, SO_RCVLOWAT, SO_SNDTIMEO, SO_RCVTIMEO,
        SO_ERROR, SO_TYPE, SO_RTABLE, SO_PEERCRED, SO_SPLICE, SO_DOMAIN, SO_PROTOCOL,
        SOL_SOCKET, SOMAXCONN);
        assert_complete(&defs, "SO_", &so);
        let rt = assert_defines!(defs; RT_TABLEID_MAX, RT_TABLEID_BITS, RT_TABLEID_MASK);
        assert_complete(&defs, "RT_", &rt);
        let af = assert_defines!(defs;
        AF_UNSPEC, AF_UNIX, AF_LOCAL, AF_INET, AF_IMPLINK, AF_PUP, AF_CHAOS, AF_NS, AF_ISO,
        AF_OSI, AF_ECMA, AF_DATAKIT, AF_CCITT, AF_SNA, AF_DECnet, AF_DLI, AF_LAT, AF_HYLINK,
        AF_APPLETALK, AF_ROUTE, AF_LINK, AF_COIP, AF_CNT, AF_IPX, AF_INET6, AF_ISDN, AF_E164,
        AF_NATM, AF_ENCAP, AF_SIP, AF_KEY, AF_BLUETOOTH, AF_MPLS, AF_FRAME, AF_MAX);
        assert_complete(&defs, "AF_", &af);
        let pseudo = assert_defines!(defs;
        pseudo_AF_XTP, pseudo_AF_RTIP, pseudo_AF_PIP, pseudo_AF_HDRCMPLT, pseudo_AF_PFLOW,
        pseudo_AF_PIPEX);
        assert_complete(&defs, "pseudo_AF_", &pseudo);
        let pf = assert_defines!(defs;
        PF_UNSPEC, PF_LOCAL, PF_UNIX, PF_INET, PF_IMPLINK, PF_PUP, PF_CHAOS, PF_NS, PF_ISO,
        PF_OSI, PF_ECMA, PF_DATAKIT, PF_CCITT, PF_SNA, PF_DECnet, PF_DLI, PF_LAT, PF_HYLINK,
        PF_APPLETALK, PF_ROUTE, PF_LINK, PF_XTP, PF_COIP, PF_CNT, PF_IPX, PF_INET6, PF_RTIP,
        PF_PIP, PF_ISDN, PF_NATM, PF_ENCAP, PF_SIP, PF_KEY, PF_BPF, PF_BLUETOOTH, PF_MPLS,
        PF_PFLOW, PF_PIPEX, PF_FRAME, PF_MAX);
        assert_complete(&defs, "PF_", &pf);
        let shut = assert_defines!(defs; SHUT_RD, SHUT_WR, SHUT_RDWR);
        assert_complete(&defs, "SHUT_", &shut);
        let net = assert_defines!(defs;
        NET_MAXID, NET_RT_DUMP, NET_RT_FLAGS, NET_RT_IFLIST, NET_RT_STATS, NET_RT_TABLE,
        NET_RT_IFNAMES, NET_RT_SOURCE, NET_RT_MAXID, NET_UNIX_INFLIGHT, NET_UNIX_DEFERRED,
        NET_UNIX_MAXID, NET_UNIX_PROTO_MAXID, NET_LINK_IFRXQ, NET_LINK_MAXID,
        NET_LINK_IFRXQ_MAXID, NET_KEY_SADB_DUMP, NET_KEY_SPD_DUMP, NET_KEY_MAXID,
        NET_BPF_BUFSIZE, NET_BPF_MAXBUFSIZE, NET_BPF_MAXID, NET_PFLOW_STATS, NET_PFLOW_MAXID);
        // The two ifrxq pressure values are continued on the next line in the C.
        assert_eq!(NET_LINK_IFRXQ_PRESSURE_RETURN, 1);
        assert_eq!(NET_LINK_IFRXQ_PRESSURE_DROP, 2);
        let net = [
            &net[..],
            &[
                "NET_LINK_IFRXQ_PRESSURE_RETURN",
                "NET_LINK_IFRXQ_PRESSURE_DROP",
            ],
        ]
        .concat();
        assert_complete(&defs, "NET_", &net);
        let unp = assert_defines!(defs; UNPCTL_RECVSPACE, UNPCTL_SENDSPACE);
        assert_complete(&defs, "UNPCTL_", &unp);
        let msg = assert_defines!(defs;
        MSG_OOB, MSG_PEEK, MSG_DONTROUTE, MSG_EOR, MSG_TRUNC, MSG_CTRUNC, MSG_WAITALL,
        MSG_DONTWAIT, MSG_BCAST, MSG_MCAST, MSG_NOSIGNAL, MSG_CMSG_CLOEXEC, MSG_WAITFORONE,
        MSG_CMSG_CLOFORK);
        assert_complete(&defs, "MSG_", &msg);
        let scm = assert_defines!(defs; SCM_RIGHTS, SCM_TIMESTAMP);
        assert_complete(&defs, "SCM_", &scm);
    }
}
/* </TESTS> */
