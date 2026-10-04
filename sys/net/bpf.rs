/*	$OpenBSD: bpf.h,v 1.78 2026/09/10 18:31:39 claudio Exp $	*/
/*	$NetBSD: bpf.h,v 1.15 1996/12/13 07:57:33 mikel Exp $	*/
/* <LICENSES> */
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
/* </LICENSES> */

//! The Berkeley packet filter, `bpf(4)`: `<net/bpf.h>`, the filter language and the device's
//! user interface (ioctls, `struct bpf_hdr`, the data-link types).
//!
//! Upstream: sys/net/bpf.h @ 3ce1f3f79392
//!
//! The ABI structures (`struct bpf_program`, `bpf_stat`, `bpf_version`, `bpf_hdr`,
//! `bpf_insn`, `bpf_dltlist`) keep the C layout: `libpcap` and `tcpdump(8)` pass and read
//! them. Constants keep their names; a constant that a structure member stores has the
//! member's type (`DLT_*` are `u32` as `bif_dlt`, the instruction codes `u16` as `code`).
//!
//! ## Deviations
//! - `struct bpf_ops` is [`BpfOps<P>`], generic over the packet type the three loads read
//!   (`P` is the C's `const void *`); a load answers `Option<u32>` where the C returns a value
//!   and sets `*err` (`None` is `*err = 1`).
//! - The LP64 holes of `struct bpf_program` and `struct bpf_dltlist` (after the `u_int`,
//!   before the pointer) and the two tail bytes of `struct bpf_hdr` are explicit members
//!   (`_pad0`), so every byte of a value is initialised (`AbiPod`).
//! - The user pointers `bf_insns` and `bfl_list` are `usize` user addresses.
//! - `BPF_CLASS(code)`, `BPF_SIZE`, `BPF_MODE`, `BPF_OP`, `BPF_SRC`, `BPF_RVAL`,
//!   `BPF_MISCOP`, `BPF_WORDALIGN`, `BPF_STMT` and `BPF_JUMP` are `const fn`s with the
//!   lowercase names.
//! - The userland prototypes (`bpf_filter`, `_bpf_filter`) are libpcap's, not ported.

use core::mem::size_of;
use core::sync::atomic::AtomicI32;

use crate::machine::copy::AbiPod;
use crate::net::if_::Ifreq;
use crate::sys::ioccom::{_io, _ior, _iow, _iowr};
use crate::sys::time::Timeval;

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

/// \[a\] `bpf_maxbufsize`: the largest buffer `BIOCSBLEN` grants (`net.bpf.maxbufsize`);
/// `bpf_validate` bounds packet offsets by it.
#[allow(non_upper_case_globals)] // `BPF_MAXBUFSIZE` is the default, a constant of bpf.h
pub static bpf_maxbufsize: AtomicI32 = AtomicI32::new(BPF_MAXBUFSIZE);

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

// LP64 sizes of the user-visible structures.
const _: () = assert!(size_of::<BpfProgram>() == 16);
const _: () = assert!(size_of::<BpfInsn>() == 8);
const _: () = assert!(size_of::<BpfHdr>() == 28);
const _: () = assert!(size_of::<BpfDltlist>() == 16);

#[cfg(test)]
mod tests;
