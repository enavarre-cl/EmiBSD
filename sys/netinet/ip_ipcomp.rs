/* $OpenBSD: ip_ipcomp.h,v 1.11 2020/09/01 01:53:34 gnezdo Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2001 Jean-Jacques Bernard-Gundol (jj@wabbitt.org)
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *   notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *   notice, this list of conditions and the following disclaimer in the
 *   documentation and/or other materials provided with the distribution.
 * 3. The name of the author may not be used to endorse or promote products
 *   derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! IP payload compression protocol (IPComp), see RFC 2393: `<netinet/ip_ipcomp.h>` (the
//! statistics, the header, the sysctl names), and stand-ins for the transform of
//! `netinet/ip_ipcomp.c`.
//!
//! Upstream: sys/netinet/ip_ipcomp.h @ 3ce1f3f79392
//!
//! Status: the header is `ported` (M9c); `ip_ipcomp.c` is not.
//!
//! ## Deviations
//! - `ipcompcounters` (`struct cpumem *`) is the static array of atomics `IPCOMPCOUNTERS` in
//!   `netinet/ipsec_input.rs`, which defines the C's pointer; `ipcomp_enable` is
//!   `IPCOMP_ENABLE` there too.
//! - `netinet/ip_ipcomp.c` needs `comp_algo_deflate` (`crypto/xform_ipcomp.c`, zlib's
//!   deflate), which is not ported. Its transform functions (`ipcomp_attach`, `ipcomp_init`,
//!   `ipcomp_zeroize`, `ipcomp_input`, `ipcomp_output`), which `xformsw[]` names, are
//!   stand-ins here that report themselves with `unported!`: an IPComp SA cannot be set up
//!   (`ENOSYS`), and a packet reaching the transform is dropped. `net.inet.ipcomp.enable` is
//!   0 by default, so IPComp packets go to raw sockets before they get here.

use core::sync::atomic::Ordering;

use crate::kern::uipc_mbuf::m_freem;
use crate::net::if_var::Netstack;
use crate::netinet::in_::IPPROTO_DONE;
use crate::netinet::ip_ipsp::{IpsecInit, Tdb, Xformsw};
use crate::netinet::ipsec_input::IPCOMPCOUNTERS;
use crate::sys::errno::Errno;
use crate::sys::mbuf::{Mbuf, m_freemp};
use crate::unported;

/// `struct ipcompstat`: the IPComp statistics as `net.inet.ipcomp.stats` returns them.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Ipcompstat {
    /// Packet shorter than header shows.
    pub ipcomps_hdrops: u64,
    /// Protocol family not supported.
    pub ipcomps_nopf: u64,
    /// `ipcomps_notdb`.
    pub ipcomps_notdb: u64,
    /// `ipcomps_badkcr`.
    pub ipcomps_badkcr: u64,
    /// `ipcomps_qfull`.
    pub ipcomps_qfull: u64,
    /// `ipcomps_noxform`.
    pub ipcomps_noxform: u64,
    /// `ipcomps_wrap`.
    pub ipcomps_wrap: u64,
    /// Input IPcomp packets.
    pub ipcomps_input: u64,
    /// Output IPcomp packets.
    pub ipcomps_output: u64,
    /// Trying to use an invalid TDB.
    pub ipcomps_invalid: u64,
    /// Input bytes.
    pub ipcomps_ibytes: u64,
    /// Output bytes.
    pub ipcomps_obytes: u64,
    /// Packet got larger than `IP_MAXPACKET`.
    pub ipcomps_toobig: u64,
    /// Packet blocked due to policy.
    pub ipcomps_pdrops: u64,
    /// "Crypto" processing failure.
    pub ipcomps_crypto: u64,
    /// Packets too short for compress.
    pub ipcomps_minlen: u64,
    /// Packet output failure.
    pub ipcomps_outfail: u64,
}

/// `struct ipcomp`: the IPCOMP header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ipcomp {
    /// Next header.
    pub ipcomp_nh: u8,
    /// Flags: reserved field: 0.
    pub ipcomp_flags: u8,
    /// Compression Parameter Index, network order.
    pub ipcomp_cpi: u16,
}

/// `IPCOMP_HLENGTH`: length of IPCOMP header.
pub const IPCOMP_HLENGTH: usize = 4;

// Names for IPCOMP sysctl objects

/// `IPCOMPCTL_ENABLE`: enable COMP processing.
pub const IPCOMPCTL_ENABLE: i32 = 1;
/// `IPCOMPCTL_STATS`: COMP stats.
pub const IPCOMPCTL_STATS: i32 = 2;
/// `IPCOMPCTL_MAXID`.
pub const IPCOMPCTL_MAXID: i32 = 3;

/// `enum ipcomp_counters`: one per field of [`Ipcompstat`], in the same order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum IpcompCounters {
    /// Packet shorter than header shows.
    IpcompsHdrops,
    /// Protocol family not supported.
    IpcompsNopf,
    /// `ipcomps_notdb`.
    IpcompsNotdb,
    /// `ipcomps_badkcr`.
    IpcompsBadkcr,
    /// `ipcomps_qfull`.
    IpcompsQfull,
    /// `ipcomps_noxform`.
    IpcompsNoxform,
    /// `ipcomps_wrap`.
    IpcompsWrap,
    /// Input IPcomp packets.
    IpcompsInput,
    /// Output IPcomp packets.
    IpcompsOutput,
    /// Trying to use an invalid TDB.
    IpcompsInvalid,
    /// Input bytes.
    IpcompsIbytes,
    /// Output bytes.
    IpcompsObytes,
    /// Packet got larger than `IP_MAXPACKET`.
    IpcompsToobig,
    /// Packet blocked due to policy.
    IpcompsPdrops,
    /// "Crypto" processing failure.
    IpcompsCrypto,
    /// Packets too short for compress.
    IpcompsMinlen,
    /// Packet output failure.
    IpcompsOutfail,
    /// `ipcomps_ncounters`.
    IpcompsNcounters,
}

/// `ipcompstat_inc(c)`.
pub fn ipcompstat_inc(c: IpcompCounters) {
    IPCOMPCOUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `ipcompstat_add(c, v)`.
pub fn ipcompstat_add(c: IpcompCounters, v: u64) {
    IPCOMPCOUNTERS[c as usize].fetch_add(v, Ordering::Relaxed);
}

/// `ipcomp_attach` (`netinet/ip_ipcomp.c`, not ported).
pub fn ipcomp_attach() -> i32 {
    0
}

/// `ipcomp_init` (`netinet/ip_ipcomp.c`, not ported: deflate).
pub fn ipcomp_init(
    _tdbp: &Tdb,
    _xsp: &'static Xformsw,
    _ii: &mut IpsecInit<'_>,
) -> Result<(), Errno> {
    Err(unported!("ipcomp_init (netinet/ip_ipcomp.c)"))
}

/// `ipcomp_zeroize` (`netinet/ip_ipcomp.c`, not ported): an IPComp TDB never got a
/// session, so there is nothing to free.
pub fn ipcomp_zeroize(_tdbp: &Tdb) -> Result<(), Errno> {
    Ok(())
}

/// `ipcomp_input` (`netinet/ip_ipcomp.c`, not ported): drops the packet.
pub fn ipcomp_input(
    mp: &mut Option<&'static Mbuf>,
    _tdb: &'static Tdb,
    _skip: i32,
    _protoff: i32,
    _ns: Option<&Netstack>,
) -> i32 {
    let _ = unported!("ipcomp_input (netinet/ip_ipcomp.c)");
    m_freemp(mp);
    IPPROTO_DONE
}

/// `ipcomp_output` (`netinet/ip_ipcomp.c`, not ported): drops the packet.
pub fn ipcomp_output(
    m: &'static Mbuf,
    _tdb: &'static Tdb,
    _skip: i32,
    _protoff: i32,
) -> Result<(), Errno> {
    m_freem(m);
    Err(unported!("ipcomp_output (netinet/ip_ipcomp.c)"))
}

// LP64 sizes of the C structures.
const _: () = {
    assert!(size_of::<Ipcompstat>() == IpcompCounters::IpcompsNcounters as usize * 8);
    assert!(size_of::<Ipcomp>() == IPCOMP_HLENGTH);
};
