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
 * Copyright (c) 1990, 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from the Stanford/CMU enet packet filter,
 * (net/enet.c) distributed as part of 4.3BSD, and code contributed
 * to Berkeley by Steven McCanne and Van Jacobson both of Lawrence
 * Berkeley Laboratory.
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
 *	@(#)bpf.h	8.1 (Berkeley) 6/10/93
 */
/*
 * Copyright (c) 1990, 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
 * Copyright (c) 2010, 2014 Henning Brauer <henning@openbsd.org>
 *
 * This code is derived from the Stanford/CMU enet packet filter,
 * (net/enet.c) distributed as part of 4.3BSD, and code contributed
 * to Berkeley by Steven McCanne and Van Jacobson both of Lawrence
 * Berkeley Laboratory.
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
 *	@(#)bpf.c	8.2 (Berkeley) 3/28/94
 */
/* </LICENSES> */

/* <CODE> */
//! The Berkeley packet filter, `bpf(4)`: `<net/bpf.h>` (the filter language and the
//! device's user interface: ioctls, `struct bpf_hdr`, the data-link types) and `net/bpf.c`
//! (the `/dev/bpf` device and the taps drivers call).
//!
//! Upstream: sys/net/bpf.h @ 3ce1f3f79392
//! Upstream: sys/net/bpf.c @ 3ce1f3f79392
//!
//! A driver attaches a tap (`bpfattach`: a `struct bpf_if` per interface and link type) and
//! hands its packets to `bpf_mtap` and friends while its `if_bpf` is set, which is while a
//! descriptor listens. Each open of `/dev/bpf` (a cloning device, major 23 on both
//! architectures) makes a descriptor (`struct bpf_d`, `net/bpfdesc.rs`) that `BIOCSETIF`
//! attaches to a tap; its filter (`net/bpf_filter.rs`) picks the packets and how much of
//! each to keep, and `read(2)` returns them behind `struct bpf_hdr`s.
//!
//! The ABI structures (`struct bpf_program`, `bpf_stat`, `bpf_version`, `bpf_hdr`,
//! `bpf_insn`, `bpf_dltlist`) keep the C layout: `libpcap` and `tcpdump(8)` pass and read
//! them. Constants keep their names; a constant that a structure member stores has the
//! member's type (`DLT_*` are `u32` as `bif_dlt`, the instruction codes `u16` as `code`).
//!
//! ## Deviations
//! - `struct bpf_ops` is [`BpfOps<P>`], generic over the packet type the three loads read
//!   (`P` is the C's `const void *`); a load answers `Option<u32>` where the C returns a value
//!   and sets `*err` (`None` is `*err = 1`). `bpf_mbuf_ops` is a function returning the
//!   table: its packet type carries a lifetime, which a `static` cannot.
//! - The LP64 holes of `struct bpf_program` and `struct bpf_dltlist` (after the `u_int`,
//!   before the pointer) and the two tail bytes of `struct bpf_hdr` are explicit members
//!   (`_pad0`), so every byte of a value is initialised (`AbiPod`).
//! - The user pointers `bf_insns` and `bfl_list` are `usize` user addresses.
//! - `BPF_CLASS(code)`, `BPF_SIZE`, `BPF_MODE`, `BPF_OP`, `BPF_SRC`, `BPF_RVAL`,
//!   `BPF_MISCOP`, `BPF_WORDALIGN`, `BPF_STMT`, `BPF_JUMP` and `ROTATE_BUFFERS` are `fn`s
//!   with the lowercase names.
//! - The userland prototypes (`bpf_filter`, `_bpf_filter`) are libpcap's, not ported.
//! - The chains of `struct m_hdr` that `bpf_tap_hdr`, `bpf_mtap_hdr` and `bpf_mtap_af` build
//!   on the stack (a header from a linear buffer, then the mbuf chain or a second buffer)
//!   are [`BpfPkt`]s; `bpf_mcopy`, `bpf_mbuf_copy` and the loads walk its segments. The
//!   `mp` of `_bpf_mtap` (the mbuf with the packet header) is an `Option`: `None` for
//!   `bpf_tap_hdr`, whose stack header has none, as the C's `M_PKTHDR` test finds.
//! - The taps and `bpf_mtap` return `bool` (the C's `int` "drop it"); the descriptor and tap
//!   handed to drivers stay `caddr_t` (`*mut u8`), as `if_bpf` and `if_bpf_mtap` are.
//! - SMR as in C (M11e): the taps walk `bif_dlist` (`SMR_SLIST`) and read `bd_rfilter`
//!   inside read sections, `bpf_put` frees the descriptor and `bpf_setf` the old program
//!   through `smr_call`. The deferred functions take the C's `void *`.
//! - `bpf_allocbufs` and `bpf_setf`'s copy of the program ask `malloc` for zeroed memory
//!   (`M_ZERO`), so no uninitialised byte (the padding between a `bpf_hdr` and its packet,
//!   which the C hands to user space as it is) is ever read.
//! - A descriptor lookup that the C assumes succeeds (`bpfclose`, `bpfread`, `bpfwrite`,
//!   `bpfioctl` on an open device) answers `ENXIO` when it does not.
//! - The ioctls read and write their argument through `ioctl_arg`/`ioctl_ret`;
//!   `BIOCGETIF`/`BIOCSETIF` use only the `struct ifreq`'s name (its first `IFNAMSIZ` bytes).
//! - `bpf_setf` refuses a non-NULL program of length 0 before allocating (the C allocates
//!   nothing useful and `bpf_validate` refuses it).
//! - `bpf_movein`: `MGETHDR`, `MCLGETL` and `m_tag_get` with `M_WAIT` sleep rather than
//!   fail in the C; a failure here answers `ENOBUFS`. `bpfwrite` panics on an interface
//!   without `if_output`, where the C would call NULL.
//! - `NVLAN` is not configured: `bpf_mtap_ether` passes the packet as it is. `SMALL_KERNEL`
//!   is not defined: `bpf_sysctl` is in. `KERNEL_ASSERT_LOCKED` is the kernel lock's
//!   assertion (`sys/systm.rs`; it checks with `MULTIPROCESSOR` and `DIAGNOSTIC`).

use core::cell::Cell;
use core::cmp::{max, min};
use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicI32, Ordering};

use crate::kassert;
use crate::kern::kern_event::{
    klist_free, klist_init_mutex, klist_insert, klist_invalidate, klist_remove, knote_locked,
};
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_sig::{pgsigio, sigio_free, sigio_getown, sigio_setown};
use crate::kern::kern_smr::{smr_read_enter, smr_read_leave};
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_synch::{msleep_nsec, refcnt_init, refcnt_rele, refcnt_take, wakeup};
use crate::kern::kern_sysctl::sysctl_int_bounded;
use crate::kern::kern_task::{SYSTQ, task_add, task_set};
use crate::kern::kern_tc::{microtime, nsecuptime};
use crate::kern::kern_timeout::{timeout_add_nsec, timeout_del, timeout_set};
use crate::kern::subr_prf::{Str, panic};
use crate::kern::uipc_mbuf::{MAX_LINKHDR, m_align, m_freem, m_gethdr, m_microtime};
use crate::kern::uipc_mbuf2::{m_tag_get, m_tag_prepend};
use crate::kern::vfs_subr::vdevgone;
use crate::machine::conf::{cdevsw, nchrdev};
use crate::machine::copy::{AbiPod, copyin, copyout};
use crate::machine::intr::IPL_NET;
use crate::net::bpf_filter::{_bpf_lfilter, bpf_validate};
use crate::net::bpfdesc::{BpfD, BpfDList, BpfIf, BpfIfList, BpfProgramSmr};
use crate::net::if_::{IFF_UP, IFNAMSIZ, Ifreq, ifpromisc};
use crate::net::if_var::Ifnet;
use crate::netinet::if_ether::ETHER_HDR_LEN;
use crate::sys::conf::DevTypeOpen;
use crate::sys::errno::Errno;
use crate::sys::event::{
    EVFILT_READ, FILTEROP_ISFD, FILTEROP_MPSAFE, Filterops, Kevent, Knote, knote_modify_fn,
    knote_process_fn,
};
use crate::sys::filio::{FIOASYNC, FIOGETOWN, FIONREAD, FIOSETOWN};
use crate::sys::ioccom::{_io, _ior, _iow, _iowr};
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::{M_CANFAIL, M_DEVBUF, M_NOWAIT, M_WAITOK, M_ZERO, MALLOC_MAX};
use crate::sys::mbuf::{
    M_EXT, M_FLOWID, M_PKTHDR, M_WAIT, MAXMCLBYTES, MHLEN, MT_DATA, Mbuf, PACKET_TAG_DLT, mclgetl,
    mtod,
};
use crate::sys::mutex::{mutex_assert_locked, mutex_assert_unlocked};
use crate::sys::param::PCATCH;
use crate::sys::proc::Proc;
use crate::sys::queue::{ListHead, TailqEntry, TailqHead};
use crate::sys::sigio::sigio_init;
use crate::sys::signal::{NSIG, SIGIO};
use crate::sys::smr::{SmrEntry, SmrSlistHead, smr_call, smr_init};
use crate::sys::socket::{
    AF_INET, AF_UNSPEC, NET_BPF_BUFSIZE, NET_BPF_MAXBUFSIZE, Sockaddr, SockaddrStorage,
    pseudo_AF_HDRCMPLT, sstosa,
};
use crate::sys::specdev::CLONE_SHIFT;
use crate::sys::systm::{INFSLP, MAXTSLP, kernel_assert_locked, net_lock, net_unlock};
use crate::sys::time::{Timeval, nsec_to_timeval, sec_to_nsec, timeval_to_nsec};
use crate::sys::ttycom::{TIOCGPGRP, TIOCSPGRP};
use crate::sys::types::{Dev, SaFamily, minor};
use crate::sys::uio::Uio;
use crate::sys::vnode::{IO_NDELAY, VCHR};

/// `BPF_RELEASE`: BSD style release date.
pub const BPF_RELEASE: i32 = 199606;

/// `BPF_ALIGNMENT`: what `BPF_WORDALIGN` rounds to (at least what a timeval needs).
pub const BPF_ALIGNMENT: usize = size_of::<u32>();

/// `BPF_MAXINSNS`: the longest program the kernel accepts.
pub const BPF_MAXINSNS: u32 = 512;

/// `BPF_MAXBUFSIZE`.
pub const BPF_MAXBUFSIZE: i32 = 2 * 1024 * 1024;
/// `BPF_MINBUFSIZE`.
pub const BPF_MINBUFSIZE: i32 = 32;

/// `BPF_MAJOR_VERSION`: current version number of filter architecture.
pub const BPF_MAJOR_VERSION: u16 = 1;
/// `BPF_MINOR_VERSION`.
pub const BPF_MINOR_VERSION: u16 = 1;

/// `BIOCGBLEN`.
pub const BIOCGBLEN: u64 = _ior::<u32>(b'B', 102);
/// `BIOCSBLEN`.
pub const BIOCSBLEN: u64 = _iowr::<u32>(b'B', 102);
/// `BIOCSETF`.
pub const BIOCSETF: u64 = _iow::<BpfProgram>(b'B', 103);
/// `BIOCFLUSH`.
pub const BIOCFLUSH: u64 = _io(b'B', 104);
/// `BIOCPROMISC`.
pub const BIOCPROMISC: u64 = _io(b'B', 105);
/// `BIOCGDLT`.
pub const BIOCGDLT: u64 = _ior::<u32>(b'B', 106);
/// `BIOCGETIF`.
pub const BIOCGETIF: u64 = _ior::<Ifreq>(b'B', 107);
/// `BIOCSETIF`.
pub const BIOCSETIF: u64 = _iow::<Ifreq>(b'B', 108);
/// `BIOCSRTIMEOUT`.
pub const BIOCSRTIMEOUT: u64 = _iow::<Timeval>(b'B', 109);
/// `BIOCGRTIMEOUT`.
pub const BIOCGRTIMEOUT: u64 = _ior::<Timeval>(b'B', 110);
/// `BIOCGSTATS`.
pub const BIOCGSTATS: u64 = _ior::<BpfStat>(b'B', 111);
/// `BIOCIMMEDIATE`.
pub const BIOCIMMEDIATE: u64 = _iow::<u32>(b'B', 112);
/// `BIOCVERSION`.
pub const BIOCVERSION: u64 = _ior::<BpfVersion>(b'B', 113);
/// `BIOCSRSIG`.
pub const BIOCSRSIG: u64 = _iow::<u32>(b'B', 114);
/// `BIOCGRSIG`.
pub const BIOCGRSIG: u64 = _ior::<u32>(b'B', 115);
/// `BIOCGHDRCMPLT`.
pub const BIOCGHDRCMPLT: u64 = _ior::<u32>(b'B', 116);
/// `BIOCSHDRCMPLT`.
pub const BIOCSHDRCMPLT: u64 = _iow::<u32>(b'B', 117);
/// `BIOCLOCK`.
pub const BIOCLOCK: u64 = _io(b'B', 118);
/// `BIOCSETWF`.
pub const BIOCSETWF: u64 = _iow::<BpfProgram>(b'B', 119);
/// `BIOCGFILDROP`.
pub const BIOCGFILDROP: u64 = _ior::<u32>(b'B', 120);
/// `BIOCSFILDROP`.
pub const BIOCSFILDROP: u64 = _iow::<u32>(b'B', 121);
/// `BIOCSDLT`.
pub const BIOCSDLT: u64 = _iow::<u32>(b'B', 122);
/// `BIOCGDLTLIST`.
pub const BIOCGDLTLIST: u64 = _iowr::<BpfDltlist>(b'B', 123);
/// `BIOCGDIRFILT`.
pub const BIOCGDIRFILT: u64 = _ior::<u32>(b'B', 124);
/// `BIOCSDIRFILT`.
pub const BIOCSDIRFILT: u64 = _iow::<u32>(b'B', 125);
/// `BIOCSWTIMEOUT`.
pub const BIOCSWTIMEOUT: u64 = _iow::<Timeval>(b'B', 126);
/// `BIOCGWTIMEOUT`.
pub const BIOCGWTIMEOUT: u64 = _ior::<Timeval>(b'B', 126);
/// `BIOCDWTIMEOUT`.
pub const BIOCDWTIMEOUT: u64 = _io(b'B', 126);
/// `BIOCSETFNR`.
pub const BIOCSETFNR: u64 = _iow::<BpfProgram>(b'B', 127);

/// `BPF_DIRECTION_IN`: direction filter for `BIOCSDIRFILT`/`BIOCGDIRFILT`.
pub const BPF_DIRECTION_IN: u32 = 1 << 0;
/// `BPF_DIRECTION_OUT`.
pub const BPF_DIRECTION_OUT: u32 = 1 << 1;

/// `BPF_FILDROP_PASS`: capture, pass (`BIOCGFILDROP`/`BIOCSFILDROP`).
pub const BPF_FILDROP_PASS: u8 = 0;
/// `BPF_FILDROP_CAPTURE`: capture, drop.
pub const BPF_FILDROP_CAPTURE: u8 = 1;
/// `BPF_FILDROP_DROP`: no capture, drop.
pub const BPF_FILDROP_DROP: u8 = 2;

/// `BPF_F_PRI_MASK`.
pub const BPF_F_PRI_MASK: u8 = 0x07;
/// `BPF_F_FLOWID`.
pub const BPF_F_FLOWID: u8 = 0x08;
/// `BPF_F_DIR_SHIFT`.
pub const BPF_F_DIR_SHIFT: u32 = 4;
/// `BPF_F_DIR_MASK`.
pub const BPF_F_DIR_MASK: u8 = 0x3 << BPF_F_DIR_SHIFT;
/// `BPF_F_DIR_IN`.
pub const BPF_F_DIR_IN: u8 = (BPF_DIRECTION_IN << BPF_F_DIR_SHIFT) as u8;
/// `BPF_F_DIR_OUT`.
pub const BPF_F_DIR_OUT: u8 = (BPF_DIRECTION_OUT << BPF_F_DIR_SHIFT) as u8;

/// `SIZEOF_BPF_HDR`.
pub const SIZEOF_BPF_HDR: usize = size_of::<BpfHdr>();

/// `DLT_NULL`: no link-layer encapsulation.
pub const DLT_NULL: u32 = 0;
/// `DLT_EN10MB`: Ethernet (10Mb).
pub const DLT_EN10MB: u32 = 1;
/// `DLT_EN3MB`: Experimental Ethernet (3Mb).
pub const DLT_EN3MB: u32 = 2;
/// `DLT_AX25`: Amateur Radio AX.25.
pub const DLT_AX25: u32 = 3;
/// `DLT_PRONET`: Proteon ProNET Token Ring.
pub const DLT_PRONET: u32 = 4;
/// `DLT_CHAOS`: Chaos.
pub const DLT_CHAOS: u32 = 5;
/// `DLT_IEEE802`: IEEE 802 Networks.
pub const DLT_IEEE802: u32 = 6;
/// `DLT_ARCNET`: ARCNET.
pub const DLT_ARCNET: u32 = 7;
/// `DLT_SLIP`: Serial Line IP.
pub const DLT_SLIP: u32 = 8;
/// `DLT_PPP`: Point-to-point Protocol.
pub const DLT_PPP: u32 = 9;
/// `DLT_FDDI`: FDDI.
pub const DLT_FDDI: u32 = 10;
/// `DLT_ATM_RFC1483`: LLC/SNAP encapsulated atm.
pub const DLT_ATM_RFC1483: u32 = 11;
/// `DLT_LOOP`: loopback type (af header).
pub const DLT_LOOP: u32 = 12;
/// `DLT_ENC`: IPSEC enc type (af header, spi, flags).
pub const DLT_ENC: u32 = 13;
/// `DLT_RAW`: raw IP.
pub const DLT_RAW: u32 = 14;
/// `DLT_SLIP_BSDOS`: BSD/OS Serial Line IP.
pub const DLT_SLIP_BSDOS: u32 = 15;
/// `DLT_PPP_BSDOS`: BSD/OS Point-to-point Protocol.
pub const DLT_PPP_BSDOS: u32 = 16;
/// `DLT_PFSYNC`: Packet filter state syncing.
pub const DLT_PFSYNC: u32 = 18;
/// `DLT_PPP_SERIAL`: PPP over Serial with HDLC.
pub const DLT_PPP_SERIAL: u32 = 50;
/// `DLT_PPP_ETHER`: PPP over Ethernet; session only w/o ether header.
pub const DLT_PPP_ETHER: u32 = 51;
/// `DLT_C_HDLC`: Cisco HDLC.
pub const DLT_C_HDLC: u32 = 104;
/// `DLT_IEEE802_11`: IEEE 802.11 wireless.
pub const DLT_IEEE802_11: u32 = 105;
/// `DLT_PFLOG`: Packet filter logging, by pcap people.
pub const DLT_PFLOG: u32 = 117;
/// `DLT_IEEE802_11_RADIO`: IEEE 802.11 plus WLAN header.
pub const DLT_IEEE802_11_RADIO: u32 = 127;
/// `DLT_USER0`: Reserved for private use.
pub const DLT_USER0: u32 = 147;
/// `DLT_USER1`: Reserved for private use.
pub const DLT_USER1: u32 = 148;
/// `DLT_USER2`: Reserved for private use.
pub const DLT_USER2: u32 = 149;
/// `DLT_USER3`: Reserved for private use.
pub const DLT_USER3: u32 = 150;
/// `DLT_USER4`: Reserved for private use.
pub const DLT_USER4: u32 = 151;
/// `DLT_USER5`: Reserved for private use.
pub const DLT_USER5: u32 = 152;
/// `DLT_USER6`: Reserved for private use.
pub const DLT_USER6: u32 = 153;
/// `DLT_USER7`: Reserved for private use.
pub const DLT_USER7: u32 = 154;
/// `DLT_USER8`: Reserved for private use.
pub const DLT_USER8: u32 = 155;
/// `DLT_USER9`: Reserved for private use.
pub const DLT_USER9: u32 = 156;
/// `DLT_USER10`: Reserved for private use.
pub const DLT_USER10: u32 = 157;
/// `DLT_USER11`: Reserved for private use.
pub const DLT_USER11: u32 = 158;
/// `DLT_USER12`: Reserved for private use.
pub const DLT_USER12: u32 = 159;
/// `DLT_USER13`: Reserved for private use.
pub const DLT_USER13: u32 = 160;
/// `DLT_USER14`: Reserved for private use.
pub const DLT_USER14: u32 = 161;
/// `DLT_USER15`: Reserved for private use.
pub const DLT_USER15: u32 = 162;
/// `DLT_USBPCAP`: USBPcap.
pub const DLT_USBPCAP: u32 = 249;
/// `DLT_MPLS`: MPLS Provider Edge header.
pub const DLT_MPLS: u32 = 219;
/// `DLT_OPENFLOW`: in-kernel OpenFlow, by pcap.
pub const DLT_OPENFLOW: u32 = 267;

/// `BPF_LD`: instruction class.
pub const BPF_LD: u16 = 0x00;
/// `BPF_LDX`.
pub const BPF_LDX: u16 = 0x01;
/// `BPF_ST`.
pub const BPF_ST: u16 = 0x02;
/// `BPF_STX`.
pub const BPF_STX: u16 = 0x03;
/// `BPF_ALU`.
pub const BPF_ALU: u16 = 0x04;
/// `BPF_JMP`.
pub const BPF_JMP: u16 = 0x05;
/// `BPF_RET`.
pub const BPF_RET: u16 = 0x06;
/// `BPF_MISC`.
pub const BPF_MISC: u16 = 0x07;

/// `BPF_W`: ld/ldx size.
pub const BPF_W: u16 = 0x00;
/// `BPF_H`.
pub const BPF_H: u16 = 0x08;
/// `BPF_B`.
pub const BPF_B: u16 = 0x10;
/// `BPF_IMM`: ld/ldx mode.
pub const BPF_IMM: u16 = 0x00;
/// `BPF_ABS`.
pub const BPF_ABS: u16 = 0x20;
/// `BPF_IND`.
pub const BPF_IND: u16 = 0x40;
/// `BPF_MEM`.
pub const BPF_MEM: u16 = 0x60;
/// `BPF_LEN`.
pub const BPF_LEN: u16 = 0x80;
/// `BPF_MSH`.
pub const BPF_MSH: u16 = 0xa0;
/// `BPF_RND`.
pub const BPF_RND: u16 = 0xc0;

/// `BPF_ADD`: alu/jmp operation.
pub const BPF_ADD: u16 = 0x00;
/// `BPF_SUB`.
pub const BPF_SUB: u16 = 0x10;
/// `BPF_MUL`.
pub const BPF_MUL: u16 = 0x20;
/// `BPF_DIV`.
pub const BPF_DIV: u16 = 0x30;
/// `BPF_OR`.
pub const BPF_OR: u16 = 0x40;
/// `BPF_AND`.
pub const BPF_AND: u16 = 0x50;
/// `BPF_LSH`.
pub const BPF_LSH: u16 = 0x60;
/// `BPF_RSH`.
pub const BPF_RSH: u16 = 0x70;
/// `BPF_NEG`.
pub const BPF_NEG: u16 = 0x80;
/// `BPF_MOD`.
pub const BPF_MOD: u16 = 0x90;
/// `BPF_XOR`.
pub const BPF_XOR: u16 = 0xa0;
/// `BPF_JA`.
pub const BPF_JA: u16 = 0x00;
/// `BPF_JEQ`.
pub const BPF_JEQ: u16 = 0x10;
/// `BPF_JGT`.
pub const BPF_JGT: u16 = 0x20;
/// `BPF_JGE`.
pub const BPF_JGE: u16 = 0x30;
/// `BPF_JSET`.
pub const BPF_JSET: u16 = 0x40;
/// `BPF_K`: alu/jmp source.
pub const BPF_K: u16 = 0x00;
/// `BPF_X`.
pub const BPF_X: u16 = 0x08;

/// `BPF_A`: ret source (`BPF_K` and `BPF_X` also apply).
pub const BPF_A: u16 = 0x10;

/// `BPF_TAX`: misc operation.
pub const BPF_TAX: u16 = 0x00;
/// `BPF_TXA`.
pub const BPF_TXA: u16 = 0x80;

/// `BPF_MEMWORDS`: number of scratch memory words (for `BPF_LD|BPF_MEM` and `BPF_ST`).
pub const BPF_MEMWORDS: usize = 16;

/// `NBPFILTER`: the count `config(8)` writes into `bpfilter.h` for `pseudo-device
/// bpfilter` (GENERIC).
pub const NBPFILTER: i32 = 1;

/// `BPF_BUFSIZE`: the default read buffer size.
const BPF_BUFSIZE: i32 = 32768;

/// `BPF_S_IDLE`: `bd_state` when no packet waits for a reader.
const BPF_S_IDLE: u8 = 0;
/// `BPF_S_WAIT`: a packet arrived; the wait timeout (`bd_wtout`) runs.
const BPF_S_WAIT: u8 = 1;
/// `BPF_S_DONE`: a read may return what is buffered.
const BPF_S_DONE: u8 = 2;

/// `PRINET`: interruptible.
const PRINET: i32 = 26;

/// `struct bpf_program`: the argument of `BIOCSETF`, as user space passes it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BpfProgram {
    /// `bf_len`: the number of instructions.
    pub bf_len: u32,
    /// The hole before `bf_insns`.
    pub _pad0: u32,
    /// `bf_insns`: the user address of the instructions (`struct bpf_insn *`).
    pub bf_insns: usize,
}

// SAFETY: `#[repr(C)]` integers with the hole made explicit: no padding, any bit pattern valid.
unsafe impl AbiPod for BpfProgram {}

/// `struct bpf_stat`: returned by `BIOCGSTATS`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BpfStat {
    /// `bs_recv`: number of packets received.
    pub bs_recv: u32,
    /// `bs_drop`: number of packets dropped.
    pub bs_drop: u32,
}

// SAFETY: two `u32`s: no padding, any bit pattern valid.
unsafe impl AbiPod for BpfStat {}

/// `struct bpf_version`: returned by `BIOCVERSION`. This represents the version number of the
/// filter language described by the instruction encodings below. bpf understands a program
/// iff kernel_major == filter_major && kernel_minor >= filter_minor, that is, if the value
/// returned by the running kernel has the same major number and a minor number equal to or
/// less than the filter being downloaded. Otherwise, the results are undefined, meaning an
/// error may be returned or packets may be accepted haphazardly. It has nothing to do with
/// the source code version.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BpfVersion {
    /// `bv_major`.
    pub bv_major: u16,
    /// `bv_minor`.
    pub bv_minor: u16,
}

// SAFETY: two `u16`s: no padding, any bit pattern valid.
unsafe impl AbiPod for BpfVersion {}

/// `struct bpf_timeval`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BpfTimeval {
    /// `tv_sec`.
    pub tv_sec: u32,
    /// `tv_usec`.
    pub tv_usec: u32,
}

/// `struct bpf_hdr`: the structure prepended to each packet.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BpfHdr {
    /// `bh_tstamp`: time stamp.
    pub bh_tstamp: BpfTimeval,
    /// `bh_caplen`: length of captured portion.
    pub bh_caplen: u32,
    /// `bh_datalen`: original length of packet.
    pub bh_datalen: u32,
    /// `bh_hdrlen`: length of bpf header (this struct plus alignment padding).
    pub bh_hdrlen: u16,
    /// `bh_ifidx`: receive interface index.
    pub bh_ifidx: u16,
    /// `bh_flowid`.
    pub bh_flowid: u16,
    /// `bh_flags`: `BPF_F_*`.
    pub bh_flags: u8,
    /// `bh_drops`.
    pub bh_drops: u8,
    /// `bh_csumflags`: checksum flags.
    pub bh_csumflags: u16,
    /// The C structure's tail padding.
    pub _pad0: u16,
}

// SAFETY: `#[repr(C)]` integers with the tail padding made explicit: no padding, any bit
// pattern valid.
unsafe impl AbiPod for BpfHdr {}

/// `struct bpf_insn`: the instruction data structure.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BpfInsn {
    /// `code`.
    pub code: u16,
    /// `jt`.
    pub jt: u8,
    /// `jf`.
    pub jf: u8,
    /// `k`.
    pub k: u32,
}

// SAFETY: `#[repr(C)]` integers laid out without holes (2 + 1 + 1 + 4 bytes): any bit pattern
// valid.
unsafe impl AbiPod for BpfInsn {}

/// `struct bpf_dltlist`: retrieves the available DLTs of the interface.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BpfDltlist {
    /// `bfl_len`: number of `bfl_list` array.
    pub bfl_len: u32,
    /// The hole before `bfl_list`.
    pub _pad0: u32,
    /// `bfl_list`: the user address of the array of DLTs (`u_int *`).
    pub bfl_list: usize,
}

// SAFETY: `#[repr(C)]` integers with the hole made explicit: no padding, any bit pattern valid.
unsafe impl AbiPod for BpfDltlist {}

/// `struct bpf_ops`: the load operations for `_bpf_lfilter` to use against the packet. Each
/// reads the word, half-word or byte at offset `k` in network byte order, `None` when the
/// packet is too short.
pub struct BpfOps<P: ?Sized> {
    /// `ldw`.
    pub ldw: fn(&P, u32) -> Option<u32>,
    /// `ldh`.
    pub ldh: fn(&P, u32) -> Option<u32>,
    /// `ldb`.
    pub ldb: fn(&P, u32) -> Option<u32>,
}

/// A packet as a tap hands it to the filter and to the capture: the C's chain of stack
/// `struct m_hdr`s (a header from a linear buffer) in front of an mbuf chain or of a second
/// linear buffer (`bpf_tap_hdr`, `bpf_mtap_hdr`, `bpf_mtap_af`), or an mbuf chain as it is.
#[derive(Clone, Copy)]
pub struct BpfPkt<'a> {
    /// The header the tap prepends (empty when there is none).
    hdr: &'a [u8],
    /// What follows it.
    body: BpfBody<'a>,
}

impl<'a> BpfPkt<'a> {
    /// An mbuf chain as it is (`bpf_mtap`).
    pub fn mbuf(m: &'a Mbuf) -> Self {
        Self {
            hdr: &[],
            body: BpfBody::Mbuf(m),
        }
    }

    /// The packet's bytes, segment by segment (the C's walk along `m_next`).
    fn segments(&self) -> BpfSegs<'a> {
        let (m, buf) = match self.body {
            BpfBody::Mbuf(m) => (Some(m), None),
            BpfBody::Buf(b) => (None, Some(b)),
        };
        BpfSegs {
            hdr: Some(self.hdr),
            m,
            buf,
        }
    }
}

/// The part of a [`BpfPkt`] after its header.
#[derive(Clone, Copy)]
enum BpfBody<'a> {
    /// An mbuf chain.
    Mbuf(&'a Mbuf),
    /// A linear buffer.
    Buf(&'a [u8]),
}

/// The iterator of [`BpfPkt::segments`].
struct BpfSegs<'a> {
    hdr: Option<&'a [u8]>,
    m: Option<&'a Mbuf>,
    buf: Option<&'a [u8]>,
}

/// `bpf_iflist`'s type.
pub struct BpfIflistHead(TailqHead<BpfIfList>);

// SAFETY: changed only under the kernel lock, as in C.
unsafe impl Sync for BpfIflistHead {}

impl core::ops::Deref for BpfIflistHead {
    type Target = TailqHead<BpfIfList>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// `bpf_d_list`'s type.
pub struct BpfDListHead(ListHead<BpfDList>);

// SAFETY: changed only under the kernel lock, as in C.
unsafe impl Sync for BpfDListHead {}

impl core::ops::Deref for BpfDListHead {
    type Target = ListHead<BpfDList>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// \[a\] `bpf_maxbufsize`: the largest buffer `BIOCSBLEN` grants (`net.bpf.maxbufsize`);
/// `bpf_validate` bounds packet offsets by it.
#[allow(non_upper_case_globals)] // `BPF_MAXBUFSIZE` is the default, a constant of bpf.h
pub static bpf_maxbufsize: AtomicI32 = AtomicI32::new(BPF_MAXBUFSIZE);

/// \[a\] `bpf_bufsize`: the default read buffer size (`net.bpf.bufsize`), patchable.
#[allow(non_upper_case_globals)] // `BPF_BUFSIZE` is the default, a constant of bpf.c
pub static bpf_bufsize: AtomicI32 = AtomicI32::new(BPF_BUFSIZE);

/// `bpf_iflist`: the list of interfaces; each corresponds to an ifnet.
pub static BPF_IFLIST: BpfIflistHead = BpfIflistHead(TailqHead::new());

/// `bpf_d_list`: the list of descriptors.
pub static BPF_D_LIST: BpfDListHead = BpfDListHead(ListHead::new());

/// `bpfread_filtops`.
pub static BPFREAD_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD | FILTEROP_MPSAFE,
    f_attach: None,
    f_detach: Some(filt_bpfrdetach),
    f_event: Some(filt_bpfread),
    f_modify: Some(filt_bpfreadmodify),
    f_process: Some(filt_bpfreadprocess),
};

/// `BPF_WORDALIGN(x)`: rounds `x` up to the next even multiple of `BPF_ALIGNMENT`.
pub const fn bpf_wordalign(x: usize) -> usize {
    (x + (BPF_ALIGNMENT - 1)) & !(BPF_ALIGNMENT - 1)
}

/// `BPF_CLASS(code)`: the instruction class.
pub const fn bpf_class(code: u16) -> u16 {
    code & 0x07
}

/// `BPF_SIZE(code)`: the ld/ldx size field.
pub const fn bpf_size(code: u16) -> u16 {
    code & 0x18
}

/// `BPF_MODE(code)`: the ld/ldx mode field.
pub const fn bpf_mode(code: u16) -> u16 {
    code & 0xe0
}

/// `BPF_OP(code)`: the alu/jmp operation field.
pub const fn bpf_op(code: u16) -> u16 {
    code & 0xf0
}

/// `BPF_SRC(code)`: the alu/jmp source field.
pub const fn bpf_src(code: u16) -> u16 {
    code & 0x08
}

/// `BPF_RVAL(code)`: the ret source field.
pub const fn bpf_rval(code: u16) -> u16 {
    code & 0x18
}

/// `BPF_MISCOP(code)`: the misc operation field.
pub const fn bpf_miscop(code: u16) -> u16 {
    code & 0xf8
}

/// `BPF_STMT(code, k)`: an instruction initialiser.
pub const fn bpf_stmt(code: u16, k: u32) -> BpfInsn {
    BpfInsn {
        code,
        jt: 0,
        jf: 0,
        k,
    }
}

/// `BPF_JUMP(code, k, jt, jf)`: a jump instruction initialiser.
pub const fn bpf_jump(code: u16, k: u32, jt: u8, jf: u8) -> BpfInsn {
    BpfInsn { code, jt, jf, k }
}

impl<'a> Iterator for BpfSegs<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        if let Some(h) = self.hdr.take() {
            return Some(h);
        }
        if let Some(m) = self.m {
            self.m = m.m_next().get();
            // SAFETY: an mbuf's `m_data` addresses `m_len` readable bytes of its storage, which
            // live while the chain is borrowed.
            return Some(unsafe {
                slice::from_raw_parts(mtod::<u8>(m).cast_const(), m.m_len().get() as usize)
            });
        }
        self.buf.take()
    }
}

/// `bpf_movein`: builds an mbuf of the packet a writer hands `bpfwrite` (the link header
/// first, as the descriptor's link type has it). The link header goes into `sockp` (for
/// `if_output`), the rest into the mbuf, tagged with the link type.
fn bpf_movein(
    uio: &mut Uio<'_>,
    d: &BpfD,
    bif: &BpfIf,
    sockp: &mut SockaddrStorage,
) -> Result<&'static Mbuf, Errno> {
    // Build a sockaddr based on the data link layer type. We do this at this level because
    // the ethernet header is copied directly into the data field of the sockaddr. In the case
    // of SLIP, there is no header and the packet is forwarded as is. Also, we are careful to
    // leave room at the front of the mbuf for the link level header.
    let linktype = bif.bif_dlt;
    let hlen: u32 = match linktype {
        DLT_SLIP => {
            sockp.ss_family = AF_INET;
            0
        }
        DLT_PPP => {
            sockp.ss_family = AF_UNSPEC;
            0
        }
        DLT_EN10MB => {
            sockp.ss_family = AF_UNSPEC;
            // XXX Would MAXLINKHDR be better?
            ETHER_HDR_LEN as u32
        }
        DLT_IEEE802_11 | DLT_IEEE802_11_RADIO => {
            sockp.ss_family = AF_UNSPEC;
            0
        }
        DLT_RAW | DLT_NULL => {
            sockp.ss_family = AF_UNSPEC;
            0
        }
        DLT_LOOP => {
            sockp.ss_family = AF_UNSPEC;
            size_of::<u32>() as u32
        }
        _ => return Err(Errno::EIO),
    };

    if uio.uio_resid > MAXMCLBYTES {
        return Err(Errno::EMSGSIZE);
    }
    let len = uio.uio_resid as u32;
    if len < hlen {
        return Err(Errno::EINVAL);
    }

    // Get the length of the payload so we can align it properly.
    let alen = len - hlen;

    // Allocate enough space for headers and the aligned payload.
    let max_linkhdr = MAX_LINKHDR.load(Ordering::Relaxed) as u32;
    let mlen = max(max_linkhdr, hlen) + alen.next_multiple_of(size_of::<usize>() as u32);
    if mlen as usize > MAXMCLBYTES {
        return Err(Errno::EMSGSIZE);
    }

    // MGETHDR(M_WAIT) and MCLGETL(M_WAIT) sleep rather than fail.
    let Some(m) = m_gethdr(M_WAIT, MT_DATA) else {
        return Err(Errno::ENOBUFS);
    };
    if mlen as usize > MHLEN {
        let _ = mclgetl(m, M_WAIT, mlen);
        if m.m_flags().get() & M_EXT == 0 {
            m_freem(m);
            return Err(Errno::ENOBUFS);
        }
    }

    m_align(m, alen as i32); // Align the payload.
    m.m_data().set(m.m_data().get().wrapping_sub(hlen as usize));

    m.m_pkthdr().ph_ifidx.set(0);
    m.m_pkthdr().len.set(len as i32);
    m.m_len().set(len);

    // SAFETY: `m_align` left `alen` bytes at the end of the mbuf's storage, and `mlen` kept
    // room for `hlen` more in front: `len` bytes from `m_data` are inside the storage, which
    // this function owns.
    let data = unsafe { slice::from_raw_parts_mut(mtod::<u8>(m), len as usize) };
    if let Err(e) = uiomove(data, uio) {
        m_freem(m);
        return Err(e);
    }

    smr_read_enter();
    let slen = bpf_mfilter(d.bd_wfilter.get(), &BpfPkt::mbuf(m), len);
    smr_read_leave();

    if slen < len {
        m_freem(m);
        return Err(Errno::EPERM);
    }

    // Make room for link header, and copy it to sockaddr.
    if hlen != 0 {
        if linktype == DLT_LOOP {
            // the link header indicates the address family
            kassert!(hlen as usize == size_of::<u32>());
            let af = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
            sockp.ss_family = af as SaFamily;
        } else {
            let sa_data = offset_of!(Sockaddr, sa_data);
            // SAFETY: `sockp` is a 256-byte `sockaddr_storage` of plain bytes; `sa_data` and the
            // `hlen` (at most `ETHER_HDR_LEN`) bytes after it are inside it, and `data` is a
            // distinct buffer.
            unsafe {
                ptr::copy_nonoverlapping(
                    data.as_ptr(),
                    ptr::from_mut(sockp).cast::<u8>().add(sa_data),
                    hlen as usize,
                );
            }
        }

        m.m_pkthdr().len.set(m.m_pkthdr().len.get() - hlen as i32);
        m.m_len().set(m.m_len().get() - hlen);
        m.m_data().set(m.m_data().get().wrapping_add(hlen as usize));
    }

    // Prepend the data link type as a mbuf tag.
    let Some(mtag) = m_tag_get(PACKET_TAG_DLT, size_of::<u32>() as i32, M_WAIT) else {
        m_freem(m);
        return Err(Errno::ENOBUFS);
    };
    // SAFETY: the tag's data is `sizeof(u_int)` bytes right after it (`m_tag_get`).
    unsafe { ptr::write_unaligned(mtag.data().cast::<u32>(), linktype) };
    m_tag_prepend(m, mtag);

    Ok(m)
}

/// `bpf_attachd`: attaches file to the bpf interface, i.e. make `d` listen on `bp`. Called
/// holding `bd_mtx`.
fn bpf_attachd(d: &'static BpfD, bp: &'static BpfIf) {
    mutex_assert_locked(&d.bd_mtx, "bpf_attachd");

    // Point d at bp, and add d to the interface's list of listeners. Finally, point the
    // driver's bpf cookie at the interface so it will divert packets to bpf.

    d.bd_bif.set(Some(bp));

    kernel_assert_locked();
    // SAFETY: the kernel lock is the list's; `d` is on no interface's list (`bpf_detachd`
    // took it off, or it is new) and lives until `bpf_d_smr`, a grace period after it left.
    unsafe { bp.bif_dlist.insert_head_locked(d) };

    bp.bif_driverp.set(ptr::from_ref(bp).cast_mut().cast());
}

/// `bpf_detachd`: detaches a file from its interface. Called holding `bd_mtx`, which it drops
/// around `ifpromisc`.
fn bpf_detachd(d: &'static BpfD) {
    mutex_assert_locked(&d.bd_mtx, "bpf_detachd");

    // Not attached.
    let Some(bp) = d.bd_bif.get() else {
        return;
    };

    // Remove ``d'' from the interface's descriptor list.
    kernel_assert_locked();
    // SAFETY: the kernel lock is the list's; `bpf_attachd` put `d` on `bp`'s list.
    unsafe { bp.bif_dlist.remove_locked(d) };

    if bp.bif_dlist.is_empty_locked() {
        // Let the driver know that there are no more listeners.
        bp.bif_driverp.set(ptr::null_mut());
    }

    d.bd_bif.set(None);

    // Check if this descriptor had requested promiscuous mode. If so, turn it off.
    if d.bd_promisc.get() != 0 {
        let ifp = bp.bif_ifp.get();
        kassert!(ifp.is_some());

        d.bd_promisc.set(0);

        bpf_get(d);
        mtx_leave(&d.bd_mtx);
        net_lock();
        let error = match ifp {
            Some(ifp) => ifpromisc(ifp, false),
            None => Ok(()),
        };
        net_unlock();
        mtx_enter(&d.bd_mtx);
        bpf_put(d);

        if let Err(e) = error
            && !matches!(e, Errno::EINVAL | Errno::ENODEV | Errno::ENXIO)
        {
            // Something is really wrong if we were able to put the driver into promiscuous
            // mode, but can't take it out.
            panic(format_args!("bpf: ifpromisc failed"));
        }
    }
}

/// `bpfilterattach`: the pseudo-device's attach function; descriptors are made on open.
pub fn bpfilterattach(_n: i32) {}

/// `bpfopen`: opens a bpf device. Returns `ENXIO` for illegal minor device number, `EBUSY`
/// when the descriptor cannot be allocated.
pub fn bpfopen(dev: Dev, _flag: i32, _mode: i32, _p: &Proc) -> Result<(), Errno> {
    let unit = minor(dev);

    if unit & ((1 << CLONE_SHIFT) - 1) != 0 {
        return Err(Errno::ENXIO);
    }
    let unit = unit as i32;

    kassert!(bpfilter_lookup(unit).is_none());

    // create on demand
    let Some(mem) = malloc(size_of::<BpfD>(), M_DEVBUF, M_NOWAIT | M_ZERO) else {
        return Err(Errno::EBUSY);
    };
    let bd_ptr = mem.as_ptr().cast::<BpfD>();
    // SAFETY: a fresh block of `size_of::<BpfD>()` bytes, aligned by `malloc`; nothing else
    // refers to it yet.
    unsafe { bd_ptr.write(BpfD::new()) };
    // SAFETY: initialised above; it lives until `bpf_d_smr` frees it with the last reference.
    let bd: &'static BpfD = unsafe { &*bd_ptr };

    // Mark "free" and do most initialization.
    bd.bd_unit.set(unit);
    bd.bd_bufsize.set(bpf_bufsize.load(Ordering::Relaxed));
    bd.bd_sig.set(SIGIO);
    mtx_init(&bd.bd_mtx, IPL_NET);
    task_set(&bd.bd_wake_task, bpf_wakeup_cb, bd_ptr.cast());
    timeout_set(&bd.bd_wait_tmo, bpf_wait_cb, bd_ptr.cast());
    smr_init(&bd.bd_smr);
    sigio_init(&bd.bd_sigio);
    // SAFETY: `bd_mtx` is a member of the same descriptor, which outlives its klist.
    unsafe { klist_init_mutex(&bd.bd_klist, &bd.bd_mtx) };

    bd.bd_rtout.set(0); // no timeout by default
    bd.bd_wtout.set(INFSLP); // wait for the buffer to fill by default

    refcnt_init(&bd.bd_refcnt);
    // SAFETY: a new descriptor, on no list.
    unsafe { BPF_D_LIST.insert_head(bd) };

    Ok(())
}

/// `bpfilter_lookup` for the entry points that the C runs on an open descriptor without a
/// check: `ENXIO` if there is none.
fn bpf_d_of(dev: Dev) -> Result<&'static BpfD, Errno> {
    bpfilter_lookup(minor(dev) as i32).ok_or(Errno::ENXIO)
}

/// `bpfclose`: closes the descriptor by detaching it from its interface, deallocating its
/// buffers, and marking it free.
pub fn bpfclose(dev: Dev, _flag: i32, _mode: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let d = bpf_d_of(dev)?;
    mtx_enter(&d.bd_mtx);
    bpf_detachd(d);
    bpf_wakeup(d);
    // SAFETY: `bpfopen` put `d` on `bpf_d_list`.
    unsafe { ListHead::<BpfDList>::remove(d) };
    mtx_leave(&d.bd_mtx);
    bpf_put(d);

    Ok(())
}

/// `ROTATE_BUFFERS(d)`: rotates the packet buffers in descriptor `d`. Move the store buffer
/// into the hold slot, and the free buffer into the store slot. Zero the length of the new
/// store buffer.
fn rotate_buffers(d: &BpfD) {
    kassert!(d.bd_in_uiomove.get() == 0);
    mutex_assert_locked(&d.bd_mtx, "ROTATE_BUFFERS");
    d.bd_hbuf.set(d.bd_sbuf.get());
    d.bd_hlen.set(d.bd_slen.get());
    d.bd_sbuf.set(d.bd_fbuf.get());
    d.bd_state.set(BPF_S_IDLE);
    d.bd_slen.set(0);
    d.bd_fbuf.set(ptr::null_mut());
}

/// `bpfread`: reads the next chunk of packets from the buffers.
pub fn bpfread(dev: Dev, uio: &mut Uio<'_>, ioflag: i32) -> Result<(), Errno> {
    kernel_assert_locked();

    let d = bpf_d_of(dev)?;
    if d.bd_bif.get().is_none() {
        return Err(Errno::ENXIO);
    }

    bpf_get(d);
    mtx_enter(&d.bd_mtx);

    let error = 'out: {
        // Restrict application to use a buffer the same size as as kernel buffers.
        if uio.uio_resid != d.bd_bufsize.get() as usize {
            break 'out Err(Errno::EINVAL);
        }

        // If there's a timeout, mark when the read should end.
        let mut end = 0u64;
        if d.bd_rtout.get() != 0 {
            let now = nsecuptime();
            end = now.saturating_add(d.bd_rtout.get());
        }

        // If the hold buffer is empty, then do a timed sleep, which ends when the timeout
        // expires or when enough packets have arrived to fill the store buffer.
        while d.bd_hbuf.get().is_null() {
            if d.bd_bif.get().is_none() {
                // interface is gone
                if d.bd_slen.get() == 0 {
                    break 'out Err(Errno::EIO);
                }
                rotate_buffers(d);
                break;
            }
            if d.bd_state.get() == BPF_S_DONE {
                // A packet(s) either arrived since the previous read or arrived while we were
                // asleep. Rotate the buffers and return what's here.
                rotate_buffers(d);
                break;
            }
            let error = if ioflag & IO_NDELAY != 0 {
                // User requested non-blocking I/O
                Err(Errno::EWOULDBLOCK)
            } else if d.bd_rtout.get() == 0 {
                // No read timeout set.
                d.bd_nreaders.set(d.bd_nreaders.get() + 1);
                let e = msleep_nsec(ptr::from_ref(d), &d.bd_mtx, PRINET | PCATCH, "bpf", INFSLP);
                d.bd_nreaders.set(d.bd_nreaders.get() - 1);
                e
            } else {
                let now = nsecuptime();
                if now < end {
                    // Read timeout has not expired yet.
                    d.bd_nreaders.set(d.bd_nreaders.get() + 1);
                    let e = msleep_nsec(
                        ptr::from_ref(d),
                        &d.bd_mtx,
                        PRINET | PCATCH,
                        "bpf",
                        end - now,
                    );
                    d.bd_nreaders.set(d.bd_nreaders.get() - 1);
                    e
                } else {
                    // Read timeout has expired.
                    Err(Errno::EWOULDBLOCK)
                }
            };
            match error {
                Err(Errno::EINTR | Errno::ERESTART) => break 'out error,
                Err(Errno::EWOULDBLOCK) => {
                    // On a timeout, return what's in the buffer, which may be nothing. If
                    // there is something in the store buffer, we can rotate the buffers.
                    if !d.bd_hbuf.get().is_null() {
                        // We filled up the buffer in between getting the timeout and
                        // arriving here, so we don't need to rotate.
                        break;
                    }

                    if d.bd_slen.get() == 0 {
                        break 'out Ok(());
                    }
                    rotate_buffers(d);
                    break;
                }
                _ => {}
            }
        }
        // At this point, we know we have something in the hold slot.
        let hbuf = d.bd_hbuf.get();
        let hlen = d.bd_hlen.get();
        d.bd_hbuf.set(ptr::null_mut());
        d.bd_hlen.set(0);
        d.bd_fbuf.set(ptr::null_mut());
        d.bd_in_uiomove.set(1);

        // Move data from hold buffer into user space. We know the entire buffer is
        // transferred since we checked above that the read buffer is bpf_bufsize bytes.
        mtx_leave(&d.bd_mtx);
        // SAFETY: `hbuf` is one of the descriptor's `bd_bufsize`-byte buffers (zeroed when
        // allocated), holding `hlen` bytes of packets; out of every slot while
        // `bd_in_uiomove` is set, so nothing else touches it.
        let error = uiomove(
            unsafe { slice::from_raw_parts_mut(hbuf, hlen as usize) },
            uio,
        );
        mtx_enter(&d.bd_mtx);

        // Ensure that bpf_resetd() or ROTATE_BUFFERS() haven't been called.
        kassert!(d.bd_fbuf.get().is_null());
        kassert!(d.bd_hbuf.get().is_null());
        d.bd_fbuf.set(hbuf);
        d.bd_in_uiomove.set(0);
        error
    };
    mtx_leave(&d.bd_mtx);
    bpf_put(d);

    error
}

/// `bpf_wakeup`: if there are processes sleeping on this descriptor, wakes them up.
fn bpf_wakeup(d: &'static BpfD) {
    mutex_assert_locked(&d.bd_mtx, "bpf_wakeup");

    if d.bd_nreaders.get() != 0 {
        wakeup(ptr::from_ref(d));
    }

    knote_locked(&d.bd_klist, 0);

    // As long as pgsigio() needs to be protected by the KERNEL_LOCK() we have to delay the
    // wakeup to another context to keep the hot path KERNEL_LOCK()-free.
    if d.bd_async.get() != 0 && d.bd_sig.get() != 0 {
        bpf_get(d);
        if !task_add(SYSTQ, &d.bd_wake_task) {
            bpf_put(d);
        }
    }
}

/// The descriptor a task or timeout of it was set up with (`bpfopen`).
///
/// # Safety
///
/// `xd` is the argument `bpfopen` gave `bd_wake_task` or `bd_wait_tmo`, and the reference
/// taken when the task or timeout was scheduled is still held.
unsafe fn bpf_d_arg(xd: *mut c_void) -> &'static BpfD {
    // SAFETY: the caller's contract: a live descriptor.
    unsafe { &*xd.cast::<BpfD>() }
}

/// `bpf_wakeup_cb`: `bd_wake_task`, the deferred `pgsigio`.
fn bpf_wakeup_cb(xd: *mut c_void) {
    // SAFETY: `bpf_wakeup` took a reference before adding the task.
    let d = unsafe { bpf_d_arg(xd) };

    if d.bd_async.get() != 0 && d.bd_sig.get() != 0 {
        pgsigio(&d.bd_sigio, d.bd_sig.get(), false);
    }

    bpf_put(d);
}

/// `bpf_wait_cb`: `bd_wait_tmo`, the end of the wait after the first packet.
fn bpf_wait_cb(xd: *mut c_void) {
    // SAFETY: `bpf_catchpacket` took a reference before adding the timeout.
    let d = unsafe { bpf_d_arg(xd) };

    mtx_enter(&d.bd_mtx);
    if d.bd_state.get() == BPF_S_WAIT {
        d.bd_state.set(BPF_S_DONE);
        bpf_wakeup(d);
    }
    mtx_leave(&d.bd_mtx);

    bpf_put(d);
}

/// `bpfwrite`: sends the packet a writer hands on the descriptor's interface.
pub fn bpfwrite(dev: Dev, uio: &mut Uio<'_>, _ioflag: i32) -> Result<(), Errno> {
    kernel_assert_locked();

    let d = bpf_d_of(dev)?;
    let Some(bif) = d.bd_bif.get() else {
        return Err(Errno::ENXIO);
    };

    bpf_get(d);
    let ifp = bif.bif_ifp.get();

    let error = 'out: {
        let Some(ifp) = ifp.filter(|ifp| ifp.if_flags.get() & IFF_UP != 0) else {
            break 'out Err(Errno::ENETDOWN);
        };

        if uio.uio_resid == 0 {
            break 'out Ok(());
        }

        let mut dst = SockaddrStorage::zeroed();
        let m = match bpf_movein(uio, d, bif, &mut dst) {
            Ok(m) => m,
            Err(e) => break 'out Err(e),
        };

        if m.m_pkthdr().len.get() as u32 > ifp.if_mtu.get() {
            m_freem(m);
            break 'out Err(Errno::EMSGSIZE);
        }

        m.m_pkthdr().ph_rtableid.set(ifp.if_rdomain.get());
        m.m_pkthdr().pf.prio.set(ifp.if_llprio.get());

        if d.bd_hdrcmplt.get() != 0 && dst.ss_family == AF_UNSPEC {
            dst.ss_family = pseudo_AF_HDRCMPLT;
        }

        net_lock();
        let error = match ifp.if_output.get() {
            // SAFETY: `dst` is a `sockaddr_storage` on this stack, valid for the call.
            Some(output) => unsafe { output(ifp, m, sstosa(&mut dst).cast_const(), None) },
            None => panic(format_args!("{}: no if_output", Str(&ifp.if_xname.get()))),
        };
        net_unlock();
        error
    };

    bpf_put(d);
    error
}

/// `bpf_resetd`: resets a descriptor by flushing its packet buffer and clearing the receive
/// and drop counts. Called holding `bd_mtx`.
fn bpf_resetd(d: &'static BpfD) {
    mutex_assert_locked(&d.bd_mtx, "bpf_resetd");
    kassert!(d.bd_in_uiomove.get() == 0);

    if timeout_del(&d.bd_wait_tmo) {
        bpf_put(d);
    }

    if !d.bd_hbuf.get().is_null() {
        // Free the hold buffer.
        d.bd_fbuf.set(d.bd_hbuf.get());
        d.bd_hbuf.set(ptr::null_mut());
    }
    d.bd_state.set(BPF_S_IDLE);
    d.bd_slen.set(0);
    d.bd_hlen.set(0);
    d.bd_rcount.store(0, Ordering::Relaxed);
    d.bd_dcount.set(0);
}

/// `bpf_set_wtout`: sets the wait timeout.
fn bpf_set_wtout(d: &BpfD, wtout: u64) -> Result<(), Errno> {
    mtx_enter(&d.bd_mtx);
    d.bd_wtout.set(wtout);
    mtx_leave(&d.bd_mtx);

    Ok(())
}

/// `bpf_set_wtimeout`: `BIOCSWTIMEOUT`, at most 300 seconds.
fn bpf_set_wtimeout(d: &BpfD, tv: &Timeval) -> Result<(), Errno> {
    if tv.tv_sec < 0 || !tv.is_valid() {
        return Err(Errno::EINVAL);
    }

    let nsec = timeval_to_nsec(tv);
    if nsec > sec_to_nsec(300) {
        return Err(Errno::EINVAL);
    }
    if nsec > MAXTSLP {
        return Err(Errno::EOVERFLOW);
    }

    bpf_set_wtout(d, nsec)
}

/// `bpf_get_wtimeout`: `BIOCGWTIMEOUT`; `ENXIO` when no wait timeout is set.
fn bpf_get_wtimeout(d: &BpfD) -> Result<Timeval, Errno> {
    mtx_enter(&d.bd_mtx);
    let nsec = d.bd_wtout.get();
    mtx_leave(&d.bd_mtx);

    if nsec == INFSLP {
        return Err(Errno::ENXIO);
    }

    Ok(nsec_to_timeval(nsec))
}

/// `bpfioctl`: the descriptor's ioctls.
///
/// - `FIONREAD`: check for read packet available.
/// - `BIOCGBLEN`: get buffer len \[for read()\].
/// - `BIOCSETF`: set read filter.
/// - `BIOCSETFNR`: set read filter without resetting descriptor.
/// - `BIOCFLUSH`: flush read packet buffer.
/// - `BIOCPROMISC`: put interface into promiscuous mode.
/// - `BIOCGDLTLIST`: get supported link layer types.
/// - `BIOCGDLT`: get link layer type.
/// - `BIOCSDLT`: set link layer type.
/// - `BIOCGETIF`: get interface name.
/// - `BIOCSETIF`: set interface.
/// - `BIOCSRTIMEOUT`: set read timeout.
/// - `BIOCGRTIMEOUT`: get read timeout.
/// - `BIOCSWTIMEOUT`: set wait timeout.
/// - `BIOCGWTIMEOUT`: get wait timeout.
/// - `BIOCDWTIMEOUT`: del wait timeout.
/// - `BIOCGSTATS`: get packet stats.
/// - `BIOCIMMEDIATE`: set immediate mode.
/// - `BIOCVERSION`: get filter language version.
/// - `BIOCGHDRCMPLT`: get "header already complete" flag.
/// - `BIOCSHDRCMPLT`: set "header already complete" flag.
pub fn bpfioctl(dev: Dev, cmd: u64, addr: &mut [u8], _flag: i32, _p: &Proc) -> Result<(), Errno> {
    let d = bpf_d_of(dev)?;
    if d.bd_locked.get() != 0 {
        // list of allowed ioctls when locked
        match cmd {
            BIOCGBLEN | BIOCFLUSH | BIOCGDLT | BIOCGDLTLIST | BIOCGETIF | BIOCGRTIMEOUT
            | BIOCGWTIMEOUT | BIOCGSTATS | BIOCVERSION | BIOCGRSIG | BIOCGHDRCMPLT | FIONREAD
            | BIOCLOCK | BIOCSRTIMEOUT | BIOCSWTIMEOUT | BIOCDWTIMEOUT | BIOCIMMEDIATE
            | TIOCGPGRP | BIOCGDIRFILT => {}
            _ => return Err(Errno::EPERM),
        }
    }

    bpf_get(d);

    let error = match cmd {
        // Check for read packet available.
        FIONREAD => {
            mtx_enter(&d.bd_mtx);
            let mut n = d.bd_slen.get();
            if !d.bd_hbuf.get().is_null() {
                n += d.bd_hlen.get();
            }
            mtx_leave(&d.bd_mtx);

            ioctl_ret(addr, &n);
            Ok(())
        }

        // Get buffer len [for read()].
        BIOCGBLEN => {
            ioctl_ret(addr, &(d.bd_bufsize.get() as u32));
            Ok(())
        }

        // Set buffer length.
        BIOCSBLEN => {
            if d.bd_bif.get().is_some() {
                Err(Errno::EINVAL)
            } else {
                let mut size: u32 = ioctl_arg(addr);
                let bpf_maxbufsize_local = bpf_maxbufsize.load(Ordering::Relaxed) as u32;

                if size > bpf_maxbufsize_local {
                    size = bpf_maxbufsize_local;
                    ioctl_ret(addr, &size);
                } else if size < BPF_MINBUFSIZE as u32 {
                    size = BPF_MINBUFSIZE as u32;
                    ioctl_ret(addr, &size);
                }
                mtx_enter(&d.bd_mtx);
                d.bd_bufsize.set(size as i32);
                mtx_leave(&d.bd_mtx);
                Ok(())
            }
        }

        // Set link layer read/write filter.
        BIOCSETF | BIOCSETFNR | BIOCSETWF => bpf_setf(d, &ioctl_arg::<BpfProgram>(addr), cmd),

        // Flush read packet buffer.
        BIOCFLUSH => {
            mtx_enter(&d.bd_mtx);
            bpf_resetd(d);
            mtx_leave(&d.bd_mtx);
            Ok(())
        }

        // Put interface into promiscuous mode.
        BIOCPROMISC => match d.bd_bif.get() {
            // No interface attached yet.
            None => Err(Errno::EINVAL),
            Some(bif) => match bif.bif_ifp.get() {
                Some(ifp) if d.bd_promisc.get() == 0 => {
                    mutex_assert_unlocked(&d.bd_mtx, "bpfioctl");
                    net_lock();
                    let error = ifpromisc(ifp, true);
                    net_unlock();
                    if error.is_ok() {
                        d.bd_promisc.set(1);
                    }
                    error
                }
                _ => Ok(()),
            },
        },

        // Get a list of supported device parameters.
        BIOCGDLTLIST => {
            if d.bd_bif.get().is_none() {
                Err(Errno::EINVAL)
            } else {
                let mut bfl: BpfDltlist = ioctl_arg(addr);
                let error = bpf_getdltlist(d, &mut bfl);
                ioctl_ret(addr, &bfl);
                error
            }
        }

        // Get device parameters.
        BIOCGDLT => match d.bd_bif.get() {
            None => Err(Errno::EINVAL),
            Some(bif) => {
                ioctl_ret(addr, &bif.bif_dlt);
                Ok(())
            }
        },

        // Set device parameters.
        BIOCSDLT => {
            if d.bd_bif.get().is_none() {
                Err(Errno::EINVAL)
            } else {
                mtx_enter(&d.bd_mtx);
                let error = bpf_setdlt(d, ioctl_arg(addr));
                mtx_leave(&d.bd_mtx);
                error
            }
        }

        // Set interface name.
        BIOCGETIF => match d.bd_bif.get() {
            None => Err(Errno::EINVAL),
            Some(bif) => {
                bpf_ifname(bif, addr);
                Ok(())
            }
        },

        // Set interface.
        BIOCSETIF => bpf_setif(d, addr),

        // Set read timeout.
        BIOCSRTIMEOUT => {
            let tv: Timeval = ioctl_arg(addr);
            if tv.tv_sec < 0 || !tv.is_valid() {
                Err(Errno::EINVAL)
            } else {
                let rtout = timeval_to_nsec(&tv);
                if rtout > MAXTSLP {
                    Err(Errno::EOVERFLOW)
                } else {
                    mtx_enter(&d.bd_mtx);
                    d.bd_rtout.set(rtout);
                    mtx_leave(&d.bd_mtx);
                    Ok(())
                }
            }
        }

        // Get read timeout.
        BIOCGRTIMEOUT => {
            mtx_enter(&d.bd_mtx);
            let tv = nsec_to_timeval(d.bd_rtout.get());
            mtx_leave(&d.bd_mtx);
            ioctl_ret(addr, &tv);
            Ok(())
        }

        // Get packet stats.
        BIOCGSTATS => {
            let bs = BpfStat {
                bs_recv: d.bd_rcount.load(Ordering::Relaxed) as u32,
                bs_drop: d.bd_dcount.get() as u32,
            };
            ioctl_ret(addr, &bs);
            Ok(())
        }

        // Set immediate mode.
        BIOCIMMEDIATE => bpf_set_wtout(
            d,
            if ioctl_arg::<i32>(addr) != 0 {
                0
            } else {
                INFSLP
            },
        ),

        // Wait timeout.
        BIOCSWTIMEOUT => bpf_set_wtimeout(d, &ioctl_arg(addr)),
        BIOCGWTIMEOUT => bpf_get_wtimeout(d).map(|tv| ioctl_ret(addr, &tv)),
        BIOCDWTIMEOUT => bpf_set_wtout(d, INFSLP),

        BIOCVERSION => {
            let bv = BpfVersion {
                bv_major: BPF_MAJOR_VERSION,
                bv_minor: BPF_MINOR_VERSION,
            };
            ioctl_ret(addr, &bv);
            Ok(())
        }

        // get "header already complete" flag
        BIOCGHDRCMPLT => {
            ioctl_ret(addr, &(d.bd_hdrcmplt.get() as u32));
            Ok(())
        }

        // set "header already complete" flag
        BIOCSHDRCMPLT => {
            d.bd_hdrcmplt.set(i32::from(ioctl_arg::<u32>(addr) != 0));
            Ok(())
        }

        // set "locked" flag (no reset)
        BIOCLOCK => {
            d.bd_locked.set(1);
            Ok(())
        }

        // get "filter-drop" flag
        BIOCGFILDROP => {
            ioctl_ret(addr, &u32::from(d.bd_fildrop.get()));
            Ok(())
        }

        // set "filter-drop" flag
        BIOCSFILDROP => {
            let fildrop: u32 = ioctl_arg(addr);
            match u8::try_from(fildrop) {
                Ok(f @ (BPF_FILDROP_PASS | BPF_FILDROP_CAPTURE | BPF_FILDROP_DROP)) => {
                    d.bd_fildrop.set(f);
                    Ok(())
                }
                _ => Err(Errno::EINVAL),
            }
        }

        // get direction filter
        BIOCGDIRFILT => {
            ioctl_ret(addr, &u32::from(d.bd_dirfilt.get()));
            Ok(())
        }

        // set direction filter
        BIOCSDIRFILT => {
            let dirfilt = ioctl_arg::<u32>(addr) & (BPF_DIRECTION_IN | BPF_DIRECTION_OUT);
            d.bd_dirfilt.set(dirfilt as u8);
            Ok(())
        }

        // Send signal on receive packets
        FIOASYNC => {
            d.bd_async.set(ioctl_arg(addr));
            Ok(())
        }

        // Process or group to send signals to
        FIOSETOWN | TIOCSPGRP => sigio_setown(&d.bd_sigio, cmd, &ioctl_arg(addr)),

        FIOGETOWN | TIOCGPGRP => {
            let mut owner = 0i32;
            sigio_getown(&d.bd_sigio, cmd, &mut owner);
            ioctl_ret(addr, &owner);
            Ok(())
        }

        // Set receive signal
        BIOCSRSIG => {
            let sig: u32 = ioctl_arg(addr);

            if sig >= NSIG as u32 {
                Err(Errno::EINVAL)
            } else {
                d.bd_sig.set(sig as i32);
                Ok(())
            }
        }
        BIOCGRSIG => {
            ioctl_ret(addr, &(d.bd_sig.get() as u32));
            Ok(())
        }

        _ => Err(Errno::EINVAL),
    };

    bpf_put(d);
    error
}

/// `bpf_setf`: sets `d`'s packet filter program to `fp`. If this file already has a filter,
/// free it and replace it. Returns `EINVAL` for bogus requests.
pub fn bpf_setf(d: &'static BpfD, fp: &BpfProgram, cmd: u64) -> Result<(), Errno> {
    kernel_assert_locked();

    let bps = if fp.bf_insns == 0 {
        if fp.bf_len != 0 {
            return Err(Errno::EINVAL);
        }
        None
    } else {
        let flen = fp.bf_len;
        if flen > BPF_MAXINSNS {
            return Err(Errno::EINVAL);
        }
        if flen == 0 {
            // bpf_validate() refuses an empty program.
            return Err(Errno::EINVAL);
        }

        let Some(fcode) = mallocarray(
            flen as usize,
            size_of::<BpfInsn>(),
            M_DEVBUF,
            M_WAITOK | M_CANFAIL | M_ZERO,
        ) else {
            return Err(Errno::ENOMEM);
        };

        let size = flen as usize * size_of::<BpfInsn>();
        // SAFETY: a fresh zeroed block of `size` bytes, aligned for `BpfInsn` by `malloc`;
        // only this function refers to it.
        let bytes = unsafe { slice::from_raw_parts_mut(fcode.as_ptr(), size) };
        let copied = copyin(fp.bf_insns, bytes).is_ok();
        // SAFETY: the same block, `flen` instructions of plain integers (`AbiPod`).
        let insns =
            unsafe { slice::from_raw_parts(fcode.as_ptr().cast::<BpfInsn>(), flen as usize) };
        if !copied || !bpf_validate(insns) {
            free(fcode, M_DEVBUF, size);
            return Err(Errno::EINVAL);
        }

        let Some(mem) = malloc(size_of::<BpfProgramSmr>(), M_DEVBUF, M_WAITOK) else {
            panic(format_args!("bpf_setf: malloc(M_WAITOK) failed"));
        };
        let bps_ptr = mem.as_ptr().cast::<BpfProgramSmr>();
        // SAFETY: a fresh block of the structure's size, aligned by `malloc`.
        unsafe {
            bps_ptr.write(BpfProgramSmr {
                bf_len: flen,
                bf_insns: fcode.cast(),
                bps_smr: SmrEntry::new(),
            });
        }
        // SAFETY: initialised above.
        smr_init(unsafe { &(*bps_ptr).bps_smr });
        // SAFETY: initialised above; freed by `bpf_prog_smr` once replaced or with `d`.
        Some(unsafe { &*bps_ptr })
    };

    let old_bps = if cmd != BIOCSETWF {
        d.bd_rfilter.replace(bps)
    } else {
        d.bd_wfilter.replace(bps)
    };

    if cmd == BIOCSETF {
        mtx_enter(&d.bd_mtx);
        bpf_resetd(d);
        mtx_leave(&d.bd_mtx);
    }

    if let Some(old_bps) = old_bps {
        smr_call(
            &old_bps.bps_smr,
            bpf_prog_smr,
            ptr::from_ref(old_bps).cast_mut().cast(),
        );
    }

    Ok(())
}

/// Whether two `bif_name`-style names are equal (`strcmp(a, b) == 0` on the NUL-terminated
/// strings, each at most `IFNAMSIZ` bytes).
fn bpf_name_eq(a: &[u8], b: &[u8]) -> bool {
    let cstr = |s: &[u8]| -> usize {
        let s = &s[..min(s.len(), IFNAMSIZ)];
        s.iter().position(|&c| c == 0).unwrap_or(s.len())
    };
    a[..cstr(a)] == b[..cstr(b)]
}

/// `bpf_setif`: detaches a file from its current interface (if attached at all) and attaches
/// to the interface indicated by the name stored in `ifr` (`ifr_name`, its first bytes).
/// Returns an errno or 0.
fn bpf_setif(d: &'static BpfD, ifr: &[u8]) -> Result<(), Errno> {
    // Look through attached interfaces for the named one.
    let Some(bp) = BPF_IFLIST.iter().find(|bp| bpf_name_eq(&bp.bif_name, ifr)) else {
        // Not found.
        return Err(Errno::ENXIO);
    };

    // Allocate the packet buffers if we need to. If we're already attached to requested
    // interface, just flush the buffer.
    mtx_enter(&d.bd_mtx);
    let error = 'out: {
        if d.bd_sbuf.get().is_null()
            && let Err(e) = bpf_allocbufs(d)
        {
            break 'out Err(e);
        }
        if !d.bd_bif.get().is_some_and(|b| ptr::eq(b, bp)) {
            // Detach if attached to something else.
            bpf_detachd(d);
            bpf_attachd(d, bp);
        }
        bpf_resetd(d);
        Ok(())
    };
    mtx_leave(&d.bd_mtx);
    error
}

/// `bpf_ifname`: copies the interface name to the ifreq (`ifr_name`, its first bytes).
fn bpf_ifname(bif: &BpfIf, ifr: &mut [u8]) {
    let n = min(ifr.len(), IFNAMSIZ);
    ifr[..n].copy_from_slice(&bif.bif_name[..n]);
}

/// `bpfkqfilter`: attaches a knote to the descriptor (`EVFILT_READ` only).
pub fn bpfkqfilter(dev: Dev, kn: &Knote) -> Result<(), Errno> {
    kernel_assert_locked();

    let Some(d) = bpfilter_lookup(minor(dev) as i32) else {
        return Err(Errno::ENXIO);
    };

    let klist = match kn.kn_filter().get() {
        EVFILT_READ => {
            kn.kn_fop.set(Some(&BPFREAD_FILTOPS));
            &d.bd_klist
        }
        _ => return Err(Errno::EINVAL),
    };

    bpf_get(d);
    kn.kn_hook.set(ptr::from_ref(d).cast_mut().cast());
    klist_insert(klist, kn);

    Ok(())
}

/// The descriptor a bpf knote hangs on (`kn->kn_hook`).
fn kn_bpf(kn: &Knote) -> &'static BpfD {
    // SAFETY: `bpfkqfilter` set `kn_hook` to a descriptor and took a reference on it, which
    // `filt_bpfrdetach` drops last.
    unsafe { &*kn.kn_hook.get().cast::<BpfD>() }
}

/// `filt_bpfrdetach`.
pub fn filt_bpfrdetach(kn: &Knote) {
    let d = kn_bpf(kn);

    klist_remove(&d.bd_klist, kn);
    bpf_put(d);
}

/// `filt_bpfread`: readable when the hold buffer has packets, or the store buffer when
/// the read may return it. Called with `bd_mtx` held.
pub fn filt_bpfread(kn: &Knote, _hint: i64) -> bool {
    let d = kn_bpf(kn);

    mutex_assert_locked(&d.bd_mtx, "filt_bpfread");

    kn.kn_data().set(i64::from(d.bd_hlen.get()));
    if d.bd_state.get() == BPF_S_DONE {
        kn.kn_data()
            .set(kn.kn_data().get() + i64::from(d.bd_slen.get()));
    }

    kn.kn_data().get() > 0
}

/// `filt_bpfreadmodify`.
pub fn filt_bpfreadmodify(kev: &mut Kevent, kn: &Knote) -> bool {
    let d = kn_bpf(kn);

    mtx_enter(&d.bd_mtx);
    let active = knote_modify_fn(kev, kn, filt_bpfread);
    mtx_leave(&d.bd_mtx);

    active
}

/// `filt_bpfreadprocess`.
pub fn filt_bpfreadprocess(kn: &Knote, kev: Option<&mut Kevent>) -> bool {
    let d = kn_bpf(kn);

    mtx_enter(&d.bd_mtx);
    let active = knote_process_fn(kn, kev, filt_bpfread);
    mtx_leave(&d.bd_mtx);

    active
}

/// `bpf_mcopy`: copies data from a packet into a buffer, as many bytes as `dst` holds. This
/// code is derived from `m_copydata` in `sys/uipc_mbuf.c`.
fn bpf_mcopy(src: &BpfPkt<'_>, dst: &mut [u8]) {
    let mut segs = src.segments();
    let mut done = 0;
    while done < dst.len() {
        let Some(seg) = segs.next() else {
            panic(format_args!("bpf_mcopy"));
        };
        let count = min(seg.len(), dst.len() - done);
        dst[done..done + count].copy_from_slice(&seg[..count]);
        done += count;
    }
}

/// `bpf_mtap`: an mbuf chain to the listeners of the tap `arg`; `true` if a listener asked
/// for the packet to be dropped (`BPF_FILDROP_*`).
pub fn bpf_mtap(arg: *mut u8, m: &Mbuf, direction: u32) -> bool {
    _bpf_mtap(arg, Some(m), BpfPkt::mbuf(m), direction)
}

/// The tap a driver's `if_bpf` (`caddr_t`) points at, if any.
fn bpf_if_of(arg: *mut u8) -> Option<&'static BpfIf> {
    // SAFETY: a driver's tap pointer is NULL or what `bpf_attachd` stored, a `bpf_if` that
    // lives until `bpfsdetach`, which first takes every listener off (clearing the pointer).
    unsafe { arg.cast::<BpfIf>().cast_const().as_ref() }
}

/// `_bpf_mtap`: hands `m` to the listeners of the tap `arg`; `mp` is the packet's mbuf with
/// the packet header (`None` for `bpf_tap_hdr`'s buffers, which have none). `true` if a
/// listener asked for the packet to be dropped.
pub fn _bpf_mtap(arg: *mut u8, mp: Option<&Mbuf>, m: BpfPkt<'_>, direction: u32) -> bool {
    let Some(bp) = bpf_if_of(arg) else {
        return false;
    };

    let pktlen: usize = m.segments().map(<[u8]>::len).sum();
    let mut tbh: Option<BpfHdr> = None;
    let mut drop = false;

    smr_read_enter();
    for d in bp.bif_dlist.iter() {
        d.bd_rcount.fetch_add(1, Ordering::Relaxed);

        if u32::from(d.bd_dirfilt.get()) & direction != 0 {
            continue;
        }

        let slen = bpf_mfilter(d.bd_rfilter.get(), &m, pktlen as u32);

        if slen == 0 {
            continue;
        }
        if d.bd_fildrop.get() != BPF_FILDROP_PASS {
            drop = true;
        }
        if d.bd_fildrop.get() != BPF_FILDROP_DROP {
            let hdr = *tbh.get_or_insert_with(|| bpf_tap_bh(mp, direction));

            mtx_enter(&d.bd_mtx);
            bpf_catchpacket(d, &m, pktlen, slen as usize, &hdr);
            mtx_leave(&d.bd_mtx);
        }
    }
    smr_read_leave();

    drop
}

/// The `struct bpf_hdr` template `_bpf_mtap` fills once per packet (`gothdr`).
fn bpf_tap_bh(mp: Option<&Mbuf>, direction: u32) -> BpfHdr {
    let mut tbh = BpfHdr::default();

    let tv = match mp.filter(|mp| mp.m_flags().get() & M_PKTHDR != 0) {
        Some(mp) => {
            let ph = mp.m_pkthdr();
            tbh.bh_ifidx = ph.ph_ifidx.get() as u16;
            tbh.bh_flowid = ph.ph_flowid.get();
            tbh.bh_flags = ph.pf.prio.get();
            if ph.csum_flags.get() & M_FLOWID != 0 {
                tbh.bh_flags |= BPF_F_FLOWID;
            }
            tbh.bh_csumflags = ph.csum_flags.get();

            m_microtime(mp)
        }
        None => microtime(),
    };

    tbh.bh_tstamp.tv_sec = tv.tv_sec as u32;
    tbh.bh_tstamp.tv_usec = tv.tv_usec as u32;
    tbh.bh_flags |= (direction << BPF_F_DIR_SHIFT) as u8;

    tbh
}

/// `bpf_tap_hdr`: incoming linkage from device drivers, where a data buffer should be
/// prepended by an arbitrary header.
pub fn bpf_tap_hdr(arg: *mut u8, hdr: Option<&[u8]>, buf: Option<&[u8]>, direction: u32) -> bool {
    if hdr.is_none() && buf.is_none() {
        // The C's chain is NULL, which _bpf_mtap() ignores.
        return false;
    }
    let pkt = BpfPkt {
        hdr: hdr.unwrap_or(&[]),
        body: BpfBody::Buf(buf.unwrap_or(&[])),
    };
    _bpf_mtap(arg, None, pkt, direction)
}

/// `bpf_mtap_hdr`: incoming linkage from device drivers, where we have a mbuf chain but need
/// to prepend some arbitrary header from a linear buffer.
pub fn bpf_mtap_hdr(arg: *mut u8, data: &[u8], m: &Mbuf, direction: u32) -> bool {
    let pkt = BpfPkt {
        hdr: data,
        body: BpfBody::Mbuf(m),
    };
    _bpf_mtap(arg, Some(m), pkt, direction)
}

/// `bpf_mtap_af`: incoming linkage from device drivers, where we have a mbuf chain but need
/// to prepend the address family.
pub fn bpf_mtap_af(arg: *mut u8, af: u32, m: &Mbuf, direction: u32) -> bool {
    let afh = af.to_be_bytes();

    bpf_mtap_hdr(arg, &afh, m, direction)
}

/// `bpf_mtap_ether`: incoming linkage from device drivers, where we have a mbuf chain but
/// need to prepend a VLAN encapsulation header; with `NVLAN` 0 the packet goes as it is.
pub fn bpf_mtap_ether(arg: *mut u8, m: &Mbuf, direction: u32) -> bool {
    // NVLAN > 0: a packet with M_VLANTAG gets its 802.1Q header back in front; vlan(4) is
    // not configured.
    _bpf_mtap(arg, Some(m), BpfPkt::mbuf(m), direction)
}

/// `bpf_catchpacket`: moves the packet data into the store buffer. Wakes up listeners if
/// needed. Called holding `bd_mtx`.
fn bpf_catchpacket(
    d: &'static BpfD,
    pkt: &BpfPkt<'_>,
    pktlen: usize,
    snaplen: usize,
    tbh: &BpfHdr,
) {
    let mut do_wakeup = false;

    mutex_assert_locked(&d.bd_mtx, "bpf_catchpacket");
    let Some(bif) = d.bd_bif.get() else {
        return;
    };

    let hdrlen = bif.bif_hdrlen as usize;
    let bufsize = d.bd_bufsize.get() as usize;

    // Figure out how many bytes to move. If the packet is greater or equal to the snapshot
    // length, transfer that much. Otherwise, transfer the whole packet (unless we hit the
    // buffer size limit).
    let totlen = min(hdrlen + min(snaplen, pktlen), bufsize);

    // Round up the end of the previous packet to the next longword.
    let mut curlen = bpf_wordalign(d.bd_slen.get() as usize);
    if curlen + totlen > bufsize {
        // This packet will overflow the storage buffer. Rotate the buffers if we can, then
        // wakeup any pending reads.
        if d.bd_fbuf.get().is_null() {
            // We haven't completed the previous read yet, so drop the packet.
            d.bd_dcount.set(d.bd_dcount.get() + 1);
            return;
        }

        // cancel pending wtime
        if timeout_del(&d.bd_wait_tmo) {
            bpf_put(d);
        }

        rotate_buffers(d);
        do_wakeup = true;
        curlen = 0;
    }

    // Append the bpf header.
    let mut bh = *tbh;
    bh.bh_datalen = pktlen as u32;
    bh.bh_hdrlen = hdrlen as u16;
    bh.bh_caplen = (totlen - hdrlen) as u32;
    // SAFETY: `bd_sbuf` is a `bd_bufsize`-byte buffer of the descriptor's, under `bd_mtx`;
    // `curlen + totlen <= bd_bufsize` and `totlen >= hdrlen >= SIZEOF_BPF_HDR`, so the header
    // and the captured bytes after it are inside it.
    let sbuf = unsafe { slice::from_raw_parts_mut(d.bd_sbuf.get().add(curlen), totlen) };
    // SAFETY: `sbuf` has at least `SIZEOF_BPF_HDR` bytes; the write is unaligned-safe.
    unsafe { ptr::write_unaligned(sbuf.as_mut_ptr().cast::<BpfHdr>(), bh) };

    // Copy the packet data into the store buffer and update its length.
    bpf_mcopy(pkt, &mut sbuf[hdrlen..]);
    d.bd_slen.set((curlen + totlen) as i32);

    match d.bd_wtout.get() {
        0 => {
            // Immediate mode is set. A packet arrived so any reads should be woken up.
            if d.bd_state.get() == BPF_S_IDLE {
                d.bd_state.set(BPF_S_DONE);
            }
            do_wakeup = true;
        }
        INFSLP => {}
        wtout => {
            if d.bd_state.get() == BPF_S_IDLE {
                d.bd_state.set(BPF_S_WAIT);

                bpf_get(d);
                if !timeout_add_nsec(&d.bd_wait_tmo, wtout) {
                    bpf_put(d);
                }
            }
        }
    }

    if do_wakeup {
        bpf_wakeup(d);
    }
}

/// `bpf_allocbufs`: allocates the descriptor's store and free buffers. Called holding
/// `bd_mtx`.
fn bpf_allocbufs(d: &BpfD) -> Result<(), Errno> {
    mutex_assert_locked(&d.bd_mtx, "bpf_allocbufs");

    let size = d.bd_bufsize.get() as usize;
    let Some(fbuf) = malloc(size, M_DEVBUF, M_NOWAIT | M_ZERO) else {
        return Err(Errno::ENOMEM);
    };
    d.bd_fbuf.set(fbuf.as_ptr());

    let Some(sbuf) = malloc(size, M_DEVBUF, M_NOWAIT | M_ZERO) else {
        free(fbuf, M_DEVBUF, size);
        d.bd_fbuf.set(ptr::null_mut());
        return Err(Errno::ENOMEM);
    };
    d.bd_sbuf.set(sbuf.as_ptr());

    d.bd_slen.set(0);
    d.bd_hlen.set(0);

    Ok(())
}

/// `bpf_prog_smr`: frees a program and its instructions (an `smr_call` callback).
fn bpf_prog_smr(bps_arg: *mut c_void) {
    // SAFETY: the argument is a program `bpf_setf` made, out of every descriptor and past
    // its grace period (or freed with its descriptor): this is its last use.
    let bps = unsafe { &*bps_arg.cast::<BpfProgramSmr>() };
    free(
        bps.bf_insns.cast(),
        M_DEVBUF,
        bps.bf_len as usize * size_of::<BpfInsn>(),
    );
    free(
        NonNull::from(bps).cast(),
        M_DEVBUF,
        size_of::<BpfProgramSmr>(),
    );
}

/// `bpf_d_smr`: frees a descriptor and everything it holds, once the last reference is gone
/// and no tap can still see it (an `smr_call` callback).
fn bpf_d_smr(smr: *mut c_void) {
    // SAFETY: `bpf_put` passes the descriptor whose last reference it dropped, a grace period
    // ago: nothing else refers to it.
    let bd = unsafe { &*smr.cast::<BpfD>() };
    sigio_free(&bd.bd_sigio);
    let size = bd.bd_bufsize.get() as usize;
    for buf in [bd.bd_sbuf.get(), bd.bd_hbuf.get(), bd.bd_fbuf.get()] {
        if let Some(buf) = NonNull::new(buf) {
            free(buf, M_DEVBUF, size);
        }
    }

    if let Some(bps) = bd.bd_rfilter.get() {
        bpf_prog_smr(ptr::from_ref(bps).cast_mut().cast());
    }
    if let Some(bps) = bd.bd_wfilter.get() {
        bpf_prog_smr(ptr::from_ref(bps).cast_mut().cast());
    }

    klist_free(&bd.bd_klist);
    free(NonNull::from(bd).cast(), M_DEVBUF, size_of::<BpfD>());
}

/// `bpf_get`: takes a reference on the descriptor's buffers.
fn bpf_get(bd: &BpfD) {
    refcnt_take(&bd.bd_refcnt);
}

/// `bpf_put`: drops a reference; frees buffers currently in use by a descriptor when the
/// reference count drops to zero.
fn bpf_put(bd: &'static BpfD) {
    if !refcnt_rele(&bd.bd_refcnt) {
        return;
    }

    smr_call(&bd.bd_smr, bpf_d_smr, ptr::from_ref(bd).cast_mut().cast());
}

/// `bpfsattach`: attaches a tap named `name` (at most `IFNAMSIZ` bytes) whose listeners
/// `*bpfp` will point at, for the link type `dlt` with `hdrlen` bytes of link header.
pub fn bpfsattach(
    bpfp: &'static Cell<*mut u8>,
    name: &[u8],
    dlt: u32,
    hdrlen: u32,
) -> &'static BpfIf {
    let Some(mem) = malloc(size_of::<BpfIf>(), M_DEVBUF, M_NOWAIT) else {
        panic(format_args!("bpfattach"));
    };
    let mut bif_name = [0u8; IFNAMSIZ];
    let n = name
        .iter()
        .take(IFNAMSIZ)
        .position(|&c| c == 0)
        .unwrap_or(min(name.len(), IFNAMSIZ));
    bif_name[..n].copy_from_slice(&name[..n]);

    let bp_ptr = mem.as_ptr().cast::<BpfIf>();
    // SAFETY: a fresh block of the structure's size, aligned by `malloc`.
    unsafe {
        bp_ptr.write(BpfIf {
            bif_next: TailqEntry::new(),
            bif_dlist: SmrSlistHead::new(),
            bif_driverp: bpfp,
            bif_dlt: dlt,
            // Compute the length of the bpf header. This is not necessarily equal to
            // SIZEOF_BPF_HDR because we want to insert spacing such that the network layer
            // header begins on a longword boundary (for performance reasons and to
            // alleviate alignment restrictions).
            bif_hdrlen: (bpf_wordalign(hdrlen as usize + SIZEOF_BPF_HDR) - hdrlen as usize) as u32,
            bif_name,
            bif_ifp: Cell::new(None),
        });
    }
    // SAFETY: initialised above; freed by `bpfsdetach` after it leaves `bpf_iflist`.
    let bp: &'static BpfIf = unsafe { &*bp_ptr };

    // SAFETY: a new tap, on no list.
    unsafe { BPF_IFLIST.insert_tail(bp) };

    bp.bif_driverp.set(ptr::null_mut());

    bp
}

/// `bpfxattach`: `bpfsattach` for the interface `ifp`.
pub fn bpfxattach(
    driverp: &'static Cell<*mut u8>,
    name: &[u8],
    ifp: &'static Ifnet,
    dlt: u32,
    hdrlen: u32,
) -> &'static BpfIf {
    let bp = bpfsattach(driverp, name, dlt, hdrlen);
    bp.bif_ifp.set(Some(ifp));

    bp
}

/// `bpfattach`: attaches the interface `ifp` to bpf under its own name; `driverp` is its
/// `if_bpf` (or another tap pointer of the driver's).
pub fn bpfattach(driverp: &'static Cell<*mut u8>, ifp: &'static Ifnet, dlt: u32, hdrlen: u32) {
    let _ = bpfxattach(driverp, &ifp.if_xname.get(), ifp, dlt, hdrlen);
}

/// `bpfdetach`: detaches an interface from its attached bpf device.
pub fn bpfdetach(ifp: &Ifnet) {
    kernel_assert_locked();

    // TAILQ_FOREACH_SAFE: each pass detaches one tap and looks again from the start.
    while let Some(bp) = BPF_IFLIST
        .iter()
        .find(|bp| bp.bif_ifp.get().is_some_and(|i| ptr::eq(i, ifp)))
    {
        bpfsdetach(bp);
    }
    ifp.if_bpf.set(ptr::null_mut());
}

/// `bpfsdetach`: detaches a tap: revokes every descriptor listening on it, then frees it.
pub fn bpfsdetach(bp: &'static BpfIf) {
    kernel_assert_locked();

    // Locate the major number.
    let maj = (0..nchrdev())
        .find(|&maj| ptr::fn_addr_eq(cdevsw(maj).d_open, bpfopen as DevTypeOpen))
        .unwrap_or(nchrdev());

    while let Some(bd) = bp.bif_dlist.first_locked() {
        bpf_get(bd);
        let unit = bd.bd_unit.get() as u32;
        vdevgone(maj, unit, unit, VCHR);
        klist_invalidate(&bd.bd_klist);
        bpf_put(bd);
    }

    // SAFETY: `bpfsattach` put `bp` on `bpf_iflist`.
    unsafe { BPF_IFLIST.remove(bp) };

    free(NonNull::from(bp).cast(), M_DEVBUF, size_of::<BpfIf>());
}

/// `bpf_sysctl`: `net.bpf.bufsize` and `net.bpf.maxbufsize`.
pub fn bpf_sysctl(
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
        NET_BPF_BUFSIZE => sysctl_int_bounded(
            oldp,
            oldlenp,
            newp,
            newlen,
            &bpf_bufsize,
            BPF_MINBUFSIZE,
            bpf_maxbufsize.load(Ordering::Relaxed),
        ),
        NET_BPF_MAXBUFSIZE => sysctl_int_bounded(
            oldp,
            oldlenp,
            newp,
            newlen,
            &bpf_maxbufsize,
            BPF_MINBUFSIZE,
            MALLOC_MAX as i32,
        ),
        _ => Err(Errno::EOPNOTSUPP),
    }
}

/// `bpfilter_lookup`: the open descriptor of `unit`.
pub fn bpfilter_lookup(unit: i32) -> Option<&'static BpfD> {
    kernel_assert_locked();

    BPF_D_LIST.iter().find(|bd| bd.bd_unit.get() == unit)
}

/// `bpf_getdltlist`: gets a list of available data link type of the interface.
fn bpf_getdltlist(d: &BpfD, bfl: &mut BpfDltlist) -> Result<(), Errno> {
    let Some(bif) = d.bd_bif.get() else {
        return Err(Errno::EINVAL);
    };
    let name = &bif.bif_name;
    let mut n: u32 = 0;
    let mut error = Ok(());
    for bp in BPF_IFLIST.iter() {
        if !bpf_name_eq(name, &bp.bif_name) {
            continue;
        }
        if bfl.bfl_list != 0 {
            if n >= bfl.bfl_len {
                return Err(Errno::ENOMEM);
            }
            error = copyout(
                &bp.bif_dlt.to_ne_bytes(),
                bfl.bfl_list + n as usize * size_of::<u32>(),
            );
            if error.is_err() {
                break;
            }
        }
        n += 1;
    }

    bfl.bfl_len = n;
    error
}

/// `bpf_setdlt`: sets the data link type of a BPF instance. Called holding `bd_mtx`.
fn bpf_setdlt(d: &'static BpfD, dlt: u32) -> Result<(), Errno> {
    mutex_assert_locked(&d.bd_mtx, "bpf_setdlt");
    let Some(bif) = d.bd_bif.get() else {
        return Err(Errno::EINVAL);
    };
    if bif.bif_dlt == dlt {
        return Ok(());
    }
    let name = &bif.bif_name;
    let Some(bp) = BPF_IFLIST
        .iter()
        .find(|bp| bpf_name_eq(name, &bp.bif_name) && bp.bif_dlt == dlt)
    else {
        return Err(Errno::EINVAL);
    };
    bpf_detachd(d);
    bpf_attachd(d, bp);
    bpf_resetd(d);
    Ok(())
}

/// `bpf_mbuf_copy`: copies `buf.len()` bytes at offset `off` of the packet into `buf`;
/// `false` (the C's -1) when the packet is too short.
fn bpf_mbuf_copy(m: &BpfPkt<'_>, off: u32, buf: &mut [u8]) -> bool {
    let mut off = off as usize;
    let mut done = 0;
    for seg in m.segments() {
        if off >= seg.len() {
            off -= seg.len();
            continue;
        }
        let count = min(seg.len() - off, buf.len() - done);
        buf[done..done + count].copy_from_slice(&seg[off..off + count]);
        done += count;
        if done == buf.len() {
            return true;
        }
        off = 0;
    }
    false
}

/// `bpf_mbuf_ldw`: the 32-bit word at offset `k`, in host order.
fn bpf_mbuf_ldw(m0: &BpfPkt<'_>, k: u32) -> Option<u32> {
    let mut v = [0u8; 4];
    bpf_mbuf_copy(m0, k, &mut v).then(|| u32::from_be_bytes(v))
}

/// `bpf_mbuf_ldh`: the 16-bit half-word at offset `k`, in host order.
fn bpf_mbuf_ldh(m0: &BpfPkt<'_>, k: u32) -> Option<u32> {
    let mut v = [0u8; 2];
    bpf_mbuf_copy(m0, k, &mut v).then(|| u32::from(u16::from_be_bytes(v)))
}

/// `bpf_mbuf_ldb`: the byte at offset `k`.
fn bpf_mbuf_ldb(m0: &BpfPkt<'_>, k: u32) -> Option<u32> {
    let mut v = [0u8; 1];
    bpf_mbuf_copy(m0, k, &mut v).then(|| u32::from(v[0]))
}

/// `bpf_mbuf_ops`: the loads `bpf_mfilter` runs a program with.
const fn bpf_mbuf_ops<'a>() -> BpfOps<BpfPkt<'a>> {
    BpfOps {
        ldw: bpf_mbuf_ldw,
        ldh: bpf_mbuf_ldh,
        ldb: bpf_mbuf_ldb,
    }
}

/// `bpf_mfilter`: runs the program `bf` over the packet `m` of `wirelen` bytes; how many
/// bytes to capture (`u32::MAX` for all), 0 to reject.
pub fn bpf_mfilter(bf: Option<&BpfProgramSmr>, m: &BpfPkt<'_>, wirelen: u32) -> u32 {
    // No filter means accept all.
    let Some(bf) = bf else {
        return u32::MAX;
    };

    _bpf_lfilter(Some(bf.insns()), &bpf_mbuf_ops(), m, wirelen)
}

/// Host tests: forgets the taps of the previous test's interfaces (their memory is gone).
#[cfg(test)]
pub(crate) fn bpf_test_reset() {
    BPF_IFLIST.init();
    BPF_D_LIST.init();
}

// LP64 sizes of the user-visible structures.
const _: () = assert!(size_of::<BpfProgram>() == 16);
const _: () = assert!(size_of::<BpfInsn>() == 8);
const _: () = assert!(size_of::<BpfHdr>() == 28);
const _: () = assert!(size_of::<BpfDltlist>() == 16);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use std::vec;
    use std::vec::Vec;

    use super::*;
    use crate::kern::init_main::PROC0;
    use crate::kern::uipc_mbuf::m_get;
    use crate::net::if_::tests::{setup_net, test_ifnet, test_packet};
    use crate::reftest::{assert_complete, assert_defines};
    use crate::sys::mbuf::M_DONTWAIT;
    use crate::sys::types::makedev;
    use crate::sys::uio::{Iovec, UioRw, UioSeg};

    /// A two-mbuf chain holding `bytes`, split after `split` bytes.
    fn chain(bytes: &[u8], split: usize) -> &'static Mbuf {
        let m = test_packet(&bytes[..split]);
        let n = m_get(M_DONTWAIT, MT_DATA).expect("mbuf");
        let rest = &bytes[split..];
        // SAFETY: a fresh mbuf has MLEN bytes at m_data; the test data is shorter.
        unsafe { ptr::copy_nonoverlapping(rest.as_ptr(), n.m_data().get(), rest.len()) };
        n.m_len().set(rest.len() as u32);
        m.m_next().set(Some(n));
        m.m_pkthdr().len.set(bytes.len() as i32);
        m
    }

    #[test]
    fn loads_across_segments() {
        let _g = setup_net();
        let bytes: Vec<u8> = (0u8..20).collect();
        let m = chain(&bytes, 6);
        let pkt = BpfPkt::mbuf(m);
        assert_eq!(bpf_mbuf_ldw(&pkt, 4), Some(0x0405_0607));
        assert_eq!(bpf_mbuf_ldh(&pkt, 5), Some(0x0506));
        assert_eq!(bpf_mbuf_ldb(&pkt, 19), Some(19));
        assert_eq!(bpf_mbuf_ldb(&pkt, 20), None);
        assert_eq!(bpf_mbuf_ldw(&pkt, 17), None);

        // bpf_mtap_af's header in front of the chain.
        let afh = 2u32.to_be_bytes();
        let pkt = BpfPkt {
            hdr: &afh,
            body: BpfBody::Mbuf(m),
        };
        assert_eq!(bpf_mbuf_ldw(&pkt, 0), Some(2));
        assert_eq!(bpf_mbuf_ldw(&pkt, 2), Some(0x0002_0001));
        let mut all = [0u8; 24];
        bpf_mcopy(&pkt, &mut all);
        assert_eq!(&all[4..], &bytes[..]);
        m_freem(m);

        // bpf_tap_hdr's two linear buffers.
        let pkt = BpfPkt {
            hdr: &[0xaa, 0xbb],
            body: BpfBody::Buf(&[1, 2, 3]),
        };
        assert_eq!(bpf_mbuf_ldw(&pkt, 1), Some(0xbb01_0203));
        assert_eq!(pkt.segments().map(<[u8]>::len).sum::<usize>(), 5);
    }

    #[test]
    fn names() {
        assert!(bpf_name_eq(
            b"vio0\0\0\0\0\0\0\0\0\0\0\0\0",
            b"vio0\0garbage"
        ));
        assert!(!bpf_name_eq(b"vio0", b"vio1"));
        assert!(!bpf_name_eq(b"vio", b"vio0"));
    }

    /// `ioctl` on the descriptor with an argument of type `T`.
    fn ioctl<T: AbiPod>(dev: Dev, cmd: u64, arg: &mut T) -> Result<(), Errno> {
        let mut data = vec![0u8; size_of::<T>().max(32)];
        ioctl_ret(&mut data, arg);
        let r = bpfioctl(dev, cmd, &mut data, 0, &PROC0);
        *arg = ioctl_arg(&data);
        r
    }

    /// Reads the descriptor's buffer (`bufsize` bytes, as bpfread insists), without waiting.
    fn read(dev: Dev, bufsize: usize) -> (Result<(), Errno>, Vec<u8>) {
        let mut buf = vec![0u8; bufsize];
        let mut iov = [Iovec::new()];
        iov[0].iov_base = buf.as_mut_ptr().cast();
        iov[0].iov_len = bufsize;
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: bufsize,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        let r = bpfread(dev, &mut uio, IO_NDELAY);
        let got = bufsize - uio.uio_resid;
        buf.truncate(got);
        (r, buf)
    }

    #[test]
    fn capture_through_a_descriptor() {
        let _g = setup_net();
        let ifp = test_ifnet(b"bpftest0");
        bpfattach(&ifp.if_bpf, ifp, DLT_EN10MB, ETHER_HDR_LEN as u32);
        assert!(ifp.if_bpf.get().is_null(), "no listener yet");

        let dev = makedev(23, 7 << CLONE_SHIFT);
        assert_eq!(bpfopen(makedev(23, 1), 0, 0, &PROC0), Err(Errno::ENXIO));
        bpfopen(dev, 0, 0, &PROC0).expect("bpfopen");

        let mut name = [0u8; 32];
        name[..8].copy_from_slice(b"bpftest0");
        assert_eq!(bpfioctl(dev, BIOCSETIF, &mut name, 0, &PROC0), Ok(()));
        assert!(
            !ifp.if_bpf.get().is_null(),
            "the listener points the driver at the tap"
        );
        let mut one = 1i32;
        ioctl(dev, BIOCIMMEDIATE, &mut one).expect("BIOCIMMEDIATE");
        let mut blen = 0u32;
        ioctl(dev, BIOCGBLEN, &mut blen).expect("BIOCGBLEN");
        assert_eq!(blen, 32768);
        let mut dlt = 0u32;
        ioctl(dev, BIOCGDLT, &mut dlt).expect("BIOCGDLT");
        assert_eq!(dlt, DLT_EN10MB);

        // A frame comes in: captured behind a bpf_hdr whose length aligns the IP header.
        let frame: Vec<u8> = (0u8..60).collect();
        let m = test_packet(&frame);
        m.m_pkthdr().ph_ifidx.set(3);
        assert!(!bpf_mtap_ether(ifp.if_bpf.get(), m, BPF_DIRECTION_IN));
        let mut n = 0i32;
        ioctl(dev, FIONREAD, &mut n).expect("FIONREAD");
        assert_eq!(n, 30 + 60);
        let (r, buf) = read(dev, blen as usize);
        assert_eq!(r, Ok(()));
        let bh: BpfHdr = ioctl_arg(&buf);
        assert_eq!((bh.bh_caplen, bh.bh_datalen, bh.bh_hdrlen), (60, 60, 30));
        assert_eq!(bh.bh_ifidx, 3);
        assert_eq!(bh.bh_flags & BPF_F_DIR_MASK, BPF_F_DIR_IN);
        assert_eq!(&buf[30..90], &frame[..]);
        // Nothing more: a non-blocking read returns nothing, successfully.
        let (r, buf) = read(dev, blen as usize);
        assert_eq!((r, buf.len()), (Ok(()), 0));
        assert_eq!(read(dev, 100).0, Err(Errno::EINVAL));

        // A read filter that keeps 4 bytes, then one that rejects; the counters see both.
        let keep4 = [bpf_stmt(BPF_RET | BPF_K, 4)];
        let mut prog = BpfProgram {
            bf_len: 1,
            _pad0: 0,
            bf_insns: keep4.as_ptr() as usize,
        };
        ioctl(dev, BIOCSETF, &mut prog).expect("BIOCSETF");
        assert!(!bpf_mtap_ether(ifp.if_bpf.get(), m, BPF_DIRECTION_IN));
        let (_, buf) = read(dev, blen as usize);
        let bh: BpfHdr = ioctl_arg(&buf);
        assert_eq!((bh.bh_caplen, bh.bh_datalen), (4, 60));
        let reject = [bpf_stmt(BPF_RET | BPF_K, 0)];
        prog.bf_insns = reject.as_ptr() as usize;
        ioctl(dev, BIOCSETFNR, &mut prog).expect("BIOCSETFNR");
        assert!(!bpf_mtap_ether(ifp.if_bpf.get(), m, BPF_DIRECTION_IN));
        let mut st = BpfStat::default();
        ioctl(dev, BIOCGSTATS, &mut st).expect("BIOCGSTATS");
        assert_eq!(
            st,
            BpfStat {
                bs_recv: 2,
                bs_drop: 0
            }
        );
        let bad = [bpf_stmt(BPF_LD | BPF_MEM, 99), bpf_stmt(BPF_RET | BPF_K, 0)];
        prog.bf_len = 2;
        prog.bf_insns = bad.as_ptr() as usize;
        assert_eq!(ioctl(dev, BIOCSETF, &mut prog), Err(Errno::EINVAL));

        // Without a filter, a listener with BPF_FILDROP_DROP drops the packet uncaptured.
        let mut none = BpfProgram::default();
        ioctl(dev, BIOCSETF, &mut none).expect("BIOCSETF NULL");
        let mut drop = u32::from(BPF_FILDROP_DROP);
        ioctl(dev, BIOCSFILDROP, &mut drop).expect("BIOCSFILDROP");
        assert!(bpf_mtap_ether(ifp.if_bpf.get(), m, BPF_DIRECTION_IN));
        // The direction filter skips outgoing packets.
        let mut out = BPF_DIRECTION_OUT;
        ioctl(dev, BIOCSDIRFILT, &mut out).expect("BIOCSDIRFILT");
        assert!(!bpf_mtap_ether(ifp.if_bpf.get(), m, BPF_DIRECTION_OUT));
        m_freem(m);

        // Locked: only the read-only ioctls remain.
        ioctl(dev, BIOCLOCK, &mut one).expect("BIOCLOCK");
        assert_eq!(ioctl(dev, BIOCSFILDROP, &mut drop), Err(Errno::EPERM));
        let mut v = BpfVersion::default();
        ioctl(dev, BIOCVERSION, &mut v).expect("BIOCVERSION");
        assert_eq!((v.bv_major, v.bv_minor), (1, 1));

        bpfclose(dev, 0, 0, None).expect("bpfclose");
        assert!(ifp.if_bpf.get().is_null(), "the last listener is gone");
        assert!(bpfilter_lookup(7 << CLONE_SHIFT).is_none());
        bpfdetach(ifp);
        assert!(
            !BPF_IFLIST
                .iter()
                .any(|bp| bpf_name_eq(&bp.bif_name, b"bpftest0"))
        );
    }

    #[test]
    fn ioctl_numbers() {
        // LP64 values, as OpenBSD/amd64 and arm64 compute them.
        assert_eq!(BIOCGBLEN, 0x4004_4266);
        assert_eq!(BIOCSBLEN, 0xc004_4266);
        assert_eq!(BIOCSETF, 0x8010_4267);
        assert_eq!(BIOCFLUSH, 0x2000_4268);
        assert_eq!(BIOCGETIF, 0x4020_426b);
        assert_eq!(BIOCSRTIMEOUT, 0x8010_426d);
        assert_eq!(BIOCGSTATS, 0x4008_426f);
        assert_eq!(BIOCVERSION, 0x4004_4271);
        assert_eq!(BIOCGDLTLIST, 0xc010_427b);
        assert_eq!(BIOCSETFNR, 0x8010_427f);
    }

    #[test]
    fn header_alignment() {
        assert_eq!(bpf_wordalign(0), 0);
        assert_eq!(bpf_wordalign(1), 4);
        assert_eq!(bpf_wordalign(28), 28);
        // An Ethernet tap: the network header after hdrlen + 14 bytes lands on a longword.
        assert_eq!(bpf_wordalign(14 + SIZEOF_BPF_HDR) - 14, 30);
        assert_eq!(bpf_class(BPF_JMP | BPF_JEQ | BPF_K), BPF_JMP);
        assert_eq!(bpf_mode(BPF_LDX | BPF_B | BPF_MSH), BPF_MSH);
        assert_eq!(bpf_op(BPF_ALU | BPF_XOR | BPF_X), BPF_XOR);
        assert_eq!(bpf_src(BPF_ALU | BPF_XOR | BPF_X), BPF_X);
        assert_eq!(bpf_size(BPF_LD | BPF_H | BPF_ABS), BPF_H);
        assert_eq!(bpf_rval(BPF_RET | BPF_A), BPF_A);
        assert_eq!(bpf_miscop(BPF_MISC | BPF_TXA), BPF_TXA);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/net/bpf.h");
        let dlt = assert_defines!(defs;
        DLT_NULL, DLT_EN10MB, DLT_EN3MB, DLT_AX25, DLT_PRONET, DLT_CHAOS, DLT_IEEE802,
        DLT_ARCNET, DLT_SLIP, DLT_PPP, DLT_FDDI, DLT_ATM_RFC1483, DLT_LOOP, DLT_ENC, DLT_RAW,
        DLT_SLIP_BSDOS, DLT_PPP_BSDOS, DLT_PFSYNC, DLT_PPP_SERIAL, DLT_PPP_ETHER, DLT_C_HDLC,
        DLT_IEEE802_11, DLT_PFLOG, DLT_IEEE802_11_RADIO, DLT_USER0, DLT_USER1, DLT_USER2,
        DLT_USER3, DLT_USER4, DLT_USER5, DLT_USER6, DLT_USER7, DLT_USER8, DLT_USER9,
        DLT_USER10, DLT_USER11, DLT_USER12, DLT_USER13, DLT_USER14, DLT_USER15, DLT_USBPCAP,
        DLT_MPLS, DLT_OPENFLOW);
        assert_complete(&defs, "DLT_", &dlt);
        assert_defines!(defs;
        BPF_RELEASE, BPF_MAXINSNS, BPF_MAXBUFSIZE, BPF_MINBUFSIZE, BPF_MAJOR_VERSION,
        BPF_MINOR_VERSION, BPF_FILDROP_PASS, BPF_FILDROP_CAPTURE, BPF_FILDROP_DROP,
        BPF_F_PRI_MASK, BPF_F_FLOWID, BPF_F_DIR_SHIFT, BPF_LD, BPF_LDX, BPF_ST, BPF_STX,
        BPF_ALU, BPF_JMP, BPF_RET, BPF_MISC, BPF_W, BPF_H, BPF_B, BPF_IMM, BPF_ABS, BPF_IND,
        BPF_MEM, BPF_LEN, BPF_MSH, BPF_RND, BPF_ADD, BPF_SUB, BPF_MUL, BPF_DIV, BPF_OR,
        BPF_AND, BPF_LSH, BPF_RSH, BPF_NEG, BPF_MOD, BPF_XOR, BPF_JA, BPF_JEQ, BPF_JGT,
        BPF_JGE, BPF_JSET, BPF_K, BPF_X, BPF_A, BPF_TAX, BPF_TXA, BPF_MEMWORDS);
    }
}
/* </TESTS> */
