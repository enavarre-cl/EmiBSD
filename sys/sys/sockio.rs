/*	$OpenBSD: sockio.h,v 1.86 2025/11/21 04:44:26 dlg Exp $	*/
/*	$NetBSD: sockio.h,v 1.5 1995/08/23 00:40:47 thorpej Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1982, 1986, 1990, 1993, 1994
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
 *	@(#)sockio.h	8.1 (Berkeley) 3/28/94
 */
/* </LICENSES> */

/* <CODE> */
//! Socket `ioctl(2)` commands: `<sys/sockio.h>`.
//!
//! Upstream: sys/sys/sockio.h @ 3ce1f3f79392
//!
//! Each command encodes its direction, group, number and the size of its argument
//! (`sys/ioccom.rs`), so the argument structures of `net/if_.rs` decide the values.
//!
//! Status: `wip`.
//!
//! ## Deviations
//! - The bridge commands (`SIOCBRDG*`, whose arguments `struct ifbreq`, `ifbrparam`,
//!   `ifbaconf`, `ifbareq`, `ifbvareq`, `ifbrpvlan`, `ifbrvidmap`, `ifbrlreq`, `ifbrlconf`,
//!   `ifbropreq` live in `<net/if_bridge.h>`) and the multicast routing counters
//!   (`SIOCGETVIFCNT`, `SIOCGETSGCNT`, `struct sioc_vif_req`/`sioc_sg_req` in
//!   `<netinet/ip_mroute.h>`) come with those headers.

use crate::net::if_::{
    IfAfreq, IfClonereq, IfLaddrreq, IfParent, IfSffpage, Ifaliasreq, Ifconf, Ifgroupreq,
    Ifkalivereq, Ifmediareq, Ifreq,
};
use crate::sys::ioccom::{_ior, _iow, _iowr};

/// At oob mark?.
pub const SIOCATMARK: u64 = _ior::<i32>(b's', 7);
/// Set process group.
pub const SIOCSPGRP: u64 = _iow::<i32>(b's', 8);
/// Get process group.
pub const SIOCGPGRP: u64 = _ior::<i32>(b's', 9);
/// Set ifnet address.
pub const SIOCSIFADDR: u64 = _iow::<Ifreq>(b'i', 12);
/// Get ifnet address.
pub const SIOCGIFADDR: u64 = _iowr::<Ifreq>(b'i', 33);
/// Set p-p address.
pub const SIOCSIFDSTADDR: u64 = _iow::<Ifreq>(b'i', 14);
/// Get p-p address.
pub const SIOCGIFDSTADDR: u64 = _iowr::<Ifreq>(b'i', 34);
/// Set ifnet flags.
pub const SIOCSIFFLAGS: u64 = _iow::<Ifreq>(b'i', 16);
/// Get ifnet flags.
pub const SIOCGIFFLAGS: u64 = _iowr::<Ifreq>(b'i', 17);
/// Get broadcast addr.
pub const SIOCGIFBRDADDR: u64 = _iowr::<Ifreq>(b'i', 35);
/// Set broadcast addr.
pub const SIOCSIFBRDADDR: u64 = _iow::<Ifreq>(b'i', 19);
/// Get ifnet list.
pub const SIOCGIFCONF: u64 = _iowr::<Ifconf>(b'i', 36);
/// Get net addr mask.
pub const SIOCGIFNETMASK: u64 = _iowr::<Ifreq>(b'i', 37);
/// Set net addr mask.
pub const SIOCSIFNETMASK: u64 = _iow::<Ifreq>(b'i', 22);
/// Get IF metric.
pub const SIOCGIFMETRIC: u64 = _iowr::<Ifreq>(b'i', 23);
/// Set IF metric.
pub const SIOCSIFMETRIC: u64 = _iow::<Ifreq>(b'i', 24);
/// Delete IF addr.
pub const SIOCDIFADDR: u64 = _iow::<Ifreq>(b'i', 25);
/// Add/chg IF alias.
pub const SIOCAIFADDR: u64 = _iow::<Ifaliasreq>(b'i', 26);
/// Get if_data.
pub const SIOCGIFDATA: u64 = _iowr::<Ifreq>(b'i', 27);
/// Set link level addr.
pub const SIOCSIFLLADDR: u64 = _iow::<Ifreq>(b'i', 31);
/// Add m'cast addr.
pub const SIOCADDMULTI: u64 = _iow::<Ifreq>(b'i', 49);
/// Del m'cast addr.
pub const SIOCDELMULTI: u64 = _iow::<Ifreq>(b'i', 50);
/// Set net media.
pub const SIOCSIFMEDIA: u64 = _iowr::<Ifreq>(b'i', 55);
/// Get net media.
pub const SIOCGIFMEDIA: u64 = _iowr::<Ifmediareq>(b'i', 56);
/// Get SFF page.
pub const SIOCGIFSFFPAGE: u64 = _iowr::<IfSffpage>(b'i', 57);
/// Delete gif addrs.
pub const SIOCDIFPHYADDR: u64 = _iow::<Ifreq>(b'i', 73);
/// Set gif addrs.
pub const SIOCSLIFPHYADDR: u64 = _iow::<IfLaddrreq>(b'i', 74);
/// Get gif addrs.
pub const SIOCGLIFPHYADDR: u64 = _iowr::<IfLaddrreq>(b'i', 75);
/// Set ifnet mtu.
pub const SIOCSIFMTU: u64 = _iow::<Ifreq>(b'i', 127);
/// Get ifnet mtu.
pub const SIOCGIFMTU: u64 = _iowr::<Ifreq>(b'i', 126);
/// Create clone if.
pub const SIOCIFCREATE: u64 = _iow::<Ifreq>(b'i', 122);
/// Destroy clone if.
pub const SIOCIFDESTROY: u64 = _iow::<Ifreq>(b'i', 121);
/// Get cloners.
pub const SIOCIFGCLONERS: u64 = _iowr::<IfClonereq>(b'i', 120);
/// Add an ifgroup.
pub const SIOCAIFGROUP: u64 = _iow::<Ifgroupreq>(b'i', 135);
/// Get ifgroups.
pub const SIOCGIFGROUP: u64 = _iowr::<Ifgroupreq>(b'i', 136);
/// Delete ifgroup.
pub const SIOCDIFGROUP: u64 = _iow::<Ifgroupreq>(b'i', 137);
/// Get members.
pub const SIOCGIFGMEMB: u64 = _iowr::<Ifgroupreq>(b'i', 138);
/// Get ifgroup attribs.
pub const SIOCGIFGATTR: u64 = _iowr::<Ifgroupreq>(b'i', 139);
/// Set ifgroup attribs.
pub const SIOCSIFGATTR: u64 = _iow::<Ifgroupreq>(b'i', 140);
/// Get ifgroup list.
pub const SIOCGIFGLIST: u64 = _iowr::<Ifgroupreq>(b'i', 141);
/// Set ifnet descr.
pub const SIOCSIFDESCR: u64 = _iow::<Ifreq>(b'i', 128);
/// Get ifnet descr.
pub const SIOCGIFDESCR: u64 = _iowr::<Ifreq>(b'i', 129);
/// Set ifnet rtlabel.
pub const SIOCSIFRTLABEL: u64 = _iow::<Ifreq>(b'i', 130);
/// Set ifnet rtlabel.
pub const SIOCGIFRTLABEL: u64 = _iowr::<Ifreq>(b'i', 131);
/// Set vlan parent if.
pub const SIOCSETVLAN: u64 = _iow::<Ifreq>(b'i', 143);
/// Get vlan parent if.
pub const SIOCGETVLAN: u64 = _iowr::<Ifreq>(b'i', 144);
/// Set pppoe params.
pub const SIOCSSPPPPARAMS: u64 = _iow::<Ifreq>(b'i', 147);
/// Get pppoe params.
pub const SIOCGSPPPPARAMS: u64 = _iowr::<Ifreq>(b'i', 148);
/// Del MPLS label.
pub const SIOCDELLABEL: u64 = _iow::<Ifreq>(b'i', 151);
/// Get MPLS PWE3 cap.
pub const SIOCGPWE3: u64 = _iowr::<Ifreq>(b'i', 152);
/// Set MPLS label.
pub const SIOCSETLABEL: u64 = _iow::<Ifreq>(b'i', 153);
/// Get MPLS label.
pub const SIOCGETLABEL: u64 = _iow::<Ifreq>(b'i', 154);
/// Set if priority.
pub const SIOCSIFPRIORITY: u64 = _iow::<Ifreq>(b'i', 155);
/// Get if priority.
pub const SIOCGIFPRIORITY: u64 = _iowr::<Ifreq>(b'i', 156);
/// Set ifnet xflags.
pub const SIOCSIFXFLAGS: u64 = _iow::<Ifreq>(b'i', 157);
/// Get ifnet xflags.
pub const SIOCGIFXFLAGS: u64 = _iowr::<Ifreq>(b'i', 158);
/// Set ifnet VRF id.
pub const SIOCSIFRDOMAIN: u64 = _iow::<Ifreq>(b'i', 159);
/// Get ifnet VRF id.
pub const SIOCGIFRDOMAIN: u64 = _iowr::<Ifreq>(b'i', 160);
/// Set tunnel VRF id.
pub const SIOCSLIFPHYRTABLE: u64 = _iow::<Ifreq>(b'i', 161);
/// Get tunnel VRF id.
pub const SIOCGLIFPHYRTABLE: u64 = _iowr::<Ifreq>(b'i', 162);
/// `SIOCSETKALIVE`.
pub const SIOCSETKALIVE: u64 = _iow::<Ifkalivereq>(b'i', 163);
/// `SIOCGETKALIVE`.
pub const SIOCGETKALIVE: u64 = _iowr::<Ifkalivereq>(b'i', 164);
/// Get ifnet hardmtu.
pub const SIOCGIFHARDMTU: u64 = _iowr::<Ifreq>(b'i', 165);
/// Set virt net id.
pub const SIOCSVNETID: u64 = _iow::<Ifreq>(b'i', 166);
/// Get virt net id.
pub const SIOCGVNETID: u64 = _iowr::<Ifreq>(b'i', 167);
/// Set tunnel ttl.
pub const SIOCSLIFPHYTTL: u64 = _iow::<Ifreq>(b'i', 168);
/// Get tunnel ttl.
pub const SIOCGLIFPHYTTL: u64 = _iowr::<Ifreq>(b'i', 169);
/// `SIOCGIFRXR`.
pub const SIOCGIFRXR: u64 = _iow::<Ifreq>(b'i', 170);
/// Attach given af.
pub const SIOCIFAFATTACH: u64 = _iow::<IfAfreq>(b'i', 171);
/// Detach given af.
pub const SIOCIFAFDETACH: u64 = _iow::<IfAfreq>(b'i', 172);
/// Set mpw config.
pub const SIOCSETMPWCFG: u64 = _iow::<Ifreq>(b'i', 173);
/// Get mpw config.
pub const SIOCGETMPWCFG: u64 = _iowr::<Ifreq>(b'i', 174);
/// Del virt net id.
pub const SIOCDVNETID: u64 = _iow::<Ifreq>(b'i', 175);
/// Set paired if.
pub const SIOCSIFPAIR: u64 = _iow::<Ifreq>(b'i', 176);
/// Get paired if.
pub const SIOCGIFPAIR: u64 = _iowr::<Ifreq>(b'i', 177);
/// Set parent if.
pub const SIOCSIFPARENT: u64 = _iow::<IfParent>(b'i', 178);
/// Get parent if.
pub const SIOCGIFPARENT: u64 = _iowr::<IfParent>(b'i', 179);
/// Del parent if.
pub const SIOCDIFPARENT: u64 = _iow::<Ifreq>(b'i', 180);
/// Set ifnet llprio.
pub const SIOCSIFLLPRIO: u64 = _iow::<Ifreq>(b'i', 181);
/// Get ifnet llprio.
pub const SIOCGIFLLPRIO: u64 = _iowr::<Ifreq>(b'i', 182);
/// Get MBIM info.
pub const SIOCGUMBINFO: u64 = _iowr::<Ifreq>(b'i', 190);
/// Set MBIM param.
pub const SIOCSUMBPARAM: u64 = _iow::<Ifreq>(b'i', 191);
/// Get MBIM param.
pub const SIOCGUMBPARAM: u64 = _iowr::<Ifreq>(b'i', 192);
/// Set tunnel df/nodf.
pub const SIOCSLIFPHYDF: u64 = _iow::<Ifreq>(b'i', 193);
/// Set tunnel df/nodf.
pub const SIOCGLIFPHYDF: u64 = _iowr::<Ifreq>(b'i', 194);
/// Set vnet flowid.
pub const SIOCSVNETFLOWID: u64 = _iow::<Ifreq>(b'i', 195);
/// Get vnet flowid.
pub const SIOCGVNETFLOWID: u64 = _iowr::<Ifreq>(b'i', 196);
/// Set tx hdr prio.
pub const SIOCSTXHPRIO: u64 = _iow::<Ifreq>(b'i', 197);
/// Get tx hdr prio.
pub const SIOCGTXHPRIO: u64 = _iowr::<Ifreq>(b'i', 198);
/// Set ecn copying.
pub const SIOCSLIFPHYECN: u64 = _iow::<Ifreq>(b'i', 199);
/// Get ecn copying.
pub const SIOCGLIFPHYECN: u64 = _iowr::<Ifreq>(b'i', 200);
/// Set rx hdr prio.
pub const SIOCSRXHPRIO: u64 = _iow::<Ifreq>(b'i', 219);
/// Get rx hdr prio.
pub const SIOCGRXHPRIO: u64 = _iowr::<Ifreq>(b'i', 219);
/// `SIOCSPWE3CTRLWORD`.
pub const SIOCSPWE3CTRLWORD: u64 = _iow::<Ifreq>(b'i', 220);
/// `SIOCGPWE3CTRLWORD`.
pub const SIOCGPWE3CTRLWORD: u64 = _iowr::<Ifreq>(b'i', 220);
/// `SIOCSPWE3FAT`.
pub const SIOCSPWE3FAT: u64 = _iow::<Ifreq>(b'i', 221);
/// `SIOCGPWE3FAT`.
pub const SIOCGPWE3FAT: u64 = _iowr::<Ifreq>(b'i', 221);
/// `SIOCSPWE3NEIGHBOR`.
pub const SIOCSPWE3NEIGHBOR: u64 = _iow::<IfLaddrreq>(b'i', 222);
/// `SIOCGPWE3NEIGHBOR`.
pub const SIOCGPWE3NEIGHBOR: u64 = _iowr::<IfLaddrreq>(b'i', 222);
/// `SIOCDPWE3NEIGHBOR`.
pub const SIOCDPWE3NEIGHBOR: u64 = _iow::<Ifreq>(b'i', 222);
/// Set carp param.
pub const SIOCSVH: u64 = _iowr::<Ifreq>(b'i', 245);
/// Get carp param.
pub const SIOCGVH: u64 = _iowr::<Ifreq>(b'i', 246);
/// `SIOCSETPFSYNC`.
pub const SIOCSETPFSYNC: u64 = _iow::<Ifreq>(b'i', 247);
/// `SIOCGETPFSYNC`.
pub const SIOCGETPFSYNC: u64 = _iowr::<Ifreq>(b'i', 248);
/// `SIOCSETPFLOW`.
pub const SIOCSETPFLOW: u64 = _iow::<Ifreq>(b'i', 253);
/// `SIOCGETPFLOW`.
pub const SIOCGETPFLOW: u64 = _iowr::<Ifreq>(b'i', 254);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::ioccom::{IOC_IN, IOC_INOUT, IOC_OUT, IOCPARM_MASK};
    use core::mem::size_of;

    /// Every command this file defines, by name.
    const OURS: &[(&str, u64)] = &[
        ("SIOCATMARK", SIOCATMARK),
        ("SIOCSPGRP", SIOCSPGRP),
        ("SIOCGPGRP", SIOCGPGRP),
        ("SIOCSIFADDR", SIOCSIFADDR),
        ("SIOCGIFADDR", SIOCGIFADDR),
        ("SIOCSIFDSTADDR", SIOCSIFDSTADDR),
        ("SIOCGIFDSTADDR", SIOCGIFDSTADDR),
        ("SIOCSIFFLAGS", SIOCSIFFLAGS),
        ("SIOCGIFFLAGS", SIOCGIFFLAGS),
        ("SIOCGIFBRDADDR", SIOCGIFBRDADDR),
        ("SIOCSIFBRDADDR", SIOCSIFBRDADDR),
        ("SIOCGIFCONF", SIOCGIFCONF),
        ("SIOCGIFNETMASK", SIOCGIFNETMASK),
        ("SIOCSIFNETMASK", SIOCSIFNETMASK),
        ("SIOCGIFMETRIC", SIOCGIFMETRIC),
        ("SIOCSIFMETRIC", SIOCSIFMETRIC),
        ("SIOCDIFADDR", SIOCDIFADDR),
        ("SIOCAIFADDR", SIOCAIFADDR),
        ("SIOCGIFDATA", SIOCGIFDATA),
        ("SIOCSIFLLADDR", SIOCSIFLLADDR),
        ("SIOCADDMULTI", SIOCADDMULTI),
        ("SIOCDELMULTI", SIOCDELMULTI),
        ("SIOCSIFMEDIA", SIOCSIFMEDIA),
        ("SIOCGIFMEDIA", SIOCGIFMEDIA),
        ("SIOCGIFSFFPAGE", SIOCGIFSFFPAGE),
        ("SIOCDIFPHYADDR", SIOCDIFPHYADDR),
        ("SIOCSLIFPHYADDR", SIOCSLIFPHYADDR),
        ("SIOCGLIFPHYADDR", SIOCGLIFPHYADDR),
        ("SIOCSIFMTU", SIOCSIFMTU),
        ("SIOCGIFMTU", SIOCGIFMTU),
        ("SIOCIFCREATE", SIOCIFCREATE),
        ("SIOCIFDESTROY", SIOCIFDESTROY),
        ("SIOCIFGCLONERS", SIOCIFGCLONERS),
        ("SIOCAIFGROUP", SIOCAIFGROUP),
        ("SIOCGIFGROUP", SIOCGIFGROUP),
        ("SIOCDIFGROUP", SIOCDIFGROUP),
        ("SIOCGIFGMEMB", SIOCGIFGMEMB),
        ("SIOCGIFGATTR", SIOCGIFGATTR),
        ("SIOCSIFGATTR", SIOCSIFGATTR),
        ("SIOCGIFGLIST", SIOCGIFGLIST),
        ("SIOCSIFDESCR", SIOCSIFDESCR),
        ("SIOCGIFDESCR", SIOCGIFDESCR),
        ("SIOCSIFRTLABEL", SIOCSIFRTLABEL),
        ("SIOCGIFRTLABEL", SIOCGIFRTLABEL),
        ("SIOCSETVLAN", SIOCSETVLAN),
        ("SIOCGETVLAN", SIOCGETVLAN),
        ("SIOCSSPPPPARAMS", SIOCSSPPPPARAMS),
        ("SIOCGSPPPPARAMS", SIOCGSPPPPARAMS),
        ("SIOCDELLABEL", SIOCDELLABEL),
        ("SIOCGPWE3", SIOCGPWE3),
        ("SIOCSETLABEL", SIOCSETLABEL),
        ("SIOCGETLABEL", SIOCGETLABEL),
        ("SIOCSIFPRIORITY", SIOCSIFPRIORITY),
        ("SIOCGIFPRIORITY", SIOCGIFPRIORITY),
        ("SIOCSIFXFLAGS", SIOCSIFXFLAGS),
        ("SIOCGIFXFLAGS", SIOCGIFXFLAGS),
        ("SIOCSIFRDOMAIN", SIOCSIFRDOMAIN),
        ("SIOCGIFRDOMAIN", SIOCGIFRDOMAIN),
        ("SIOCSLIFPHYRTABLE", SIOCSLIFPHYRTABLE),
        ("SIOCGLIFPHYRTABLE", SIOCGLIFPHYRTABLE),
        ("SIOCSETKALIVE", SIOCSETKALIVE),
        ("SIOCGETKALIVE", SIOCGETKALIVE),
        ("SIOCGIFHARDMTU", SIOCGIFHARDMTU),
        ("SIOCSVNETID", SIOCSVNETID),
        ("SIOCGVNETID", SIOCGVNETID),
        ("SIOCSLIFPHYTTL", SIOCSLIFPHYTTL),
        ("SIOCGLIFPHYTTL", SIOCGLIFPHYTTL),
        ("SIOCGIFRXR", SIOCGIFRXR),
        ("SIOCIFAFATTACH", SIOCIFAFATTACH),
        ("SIOCIFAFDETACH", SIOCIFAFDETACH),
        ("SIOCSETMPWCFG", SIOCSETMPWCFG),
        ("SIOCGETMPWCFG", SIOCGETMPWCFG),
        ("SIOCDVNETID", SIOCDVNETID),
        ("SIOCSIFPAIR", SIOCSIFPAIR),
        ("SIOCGIFPAIR", SIOCGIFPAIR),
        ("SIOCSIFPARENT", SIOCSIFPARENT),
        ("SIOCGIFPARENT", SIOCGIFPARENT),
        ("SIOCDIFPARENT", SIOCDIFPARENT),
        ("SIOCSIFLLPRIO", SIOCSIFLLPRIO),
        ("SIOCGIFLLPRIO", SIOCGIFLLPRIO),
        ("SIOCGUMBINFO", SIOCGUMBINFO),
        ("SIOCSUMBPARAM", SIOCSUMBPARAM),
        ("SIOCGUMBPARAM", SIOCGUMBPARAM),
        ("SIOCSLIFPHYDF", SIOCSLIFPHYDF),
        ("SIOCGLIFPHYDF", SIOCGLIFPHYDF),
        ("SIOCSVNETFLOWID", SIOCSVNETFLOWID),
        ("SIOCGVNETFLOWID", SIOCGVNETFLOWID),
        ("SIOCSTXHPRIO", SIOCSTXHPRIO),
        ("SIOCGTXHPRIO", SIOCGTXHPRIO),
        ("SIOCSLIFPHYECN", SIOCSLIFPHYECN),
        ("SIOCGLIFPHYECN", SIOCGLIFPHYECN),
        ("SIOCSRXHPRIO", SIOCSRXHPRIO),
        ("SIOCGRXHPRIO", SIOCGRXHPRIO),
        ("SIOCSPWE3CTRLWORD", SIOCSPWE3CTRLWORD),
        ("SIOCGPWE3CTRLWORD", SIOCGPWE3CTRLWORD),
        ("SIOCSPWE3FAT", SIOCSPWE3FAT),
        ("SIOCGPWE3FAT", SIOCGPWE3FAT),
        ("SIOCSPWE3NEIGHBOR", SIOCSPWE3NEIGHBOR),
        ("SIOCGPWE3NEIGHBOR", SIOCGPWE3NEIGHBOR),
        ("SIOCDPWE3NEIGHBOR", SIOCDPWE3NEIGHBOR),
        ("SIOCSVH", SIOCSVH),
        ("SIOCGVH", SIOCGVH),
        ("SIOCSETPFSYNC", SIOCSETPFSYNC),
        ("SIOCGETPFSYNC", SIOCGETPFSYNC),
        ("SIOCSETPFLOW", SIOCSETPFLOW),
        ("SIOCGETPFLOW", SIOCGETPFLOW),
    ];

    /// The commands whose argument structures are not ported yet.
    const DEFERRED_PREFIXES: &[&str] = &["SIOCBRDG", "SIOCGETVIFCNT", "SIOCGETSGCNT"];

    #[test]
    fn well_known_values() {
        // The numbers ifconfig(8) and friends use on OpenBSD/amd64 and arm64.
        assert_eq!(SIOCSIFADDR, 0x8020_690c);
        assert_eq!(SIOCGIFFLAGS, 0xc020_6911);
        assert_eq!(SIOCSIFFLAGS, 0x8020_6910);
        assert_eq!(SIOCAIFADDR, 0x8040_691a);
        assert_eq!(SIOCGIFCONF, 0xc010_6924);
        assert_eq!(SIOCATMARK, 0x4004_7307);
        assert_eq!(SIOCGIFMEDIA, 0xc040_6938);
    }

    /// The size `sizeof(t)` has for the argument types `sockio.h` names.
    fn c_size(t: &str) -> usize {
        match t {
            "int" => size_of::<i32>(),
            "struct ifreq" => size_of::<Ifreq>(),
            "struct ifconf" => size_of::<Ifconf>(),
            "struct ifaliasreq" => size_of::<Ifaliasreq>(),
            "struct ifmediareq" => size_of::<Ifmediareq>(),
            "struct if_sffpage" => size_of::<IfSffpage>(),
            "struct if_laddrreq" => size_of::<IfLaddrreq>(),
            "struct if_clonereq" => size_of::<IfClonereq>(),
            "struct ifgroupreq" => size_of::<Ifgroupreq>(),
            "struct ifkalivereq" => size_of::<Ifkalivereq>(),
            "struct if_afreq" => size_of::<IfAfreq>(),
            "struct if_parent" => size_of::<IfParent>(),
            _ => panic!("unknown argument type {t}"),
        }
    }

    /// Every `#define SIOC... _IO*('g', n, type)` of the C: the direction, group, number and
    /// argument type it names must give the value we computed.
    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/sockio.h");
        let mut seen = 0;
        for (name, text) in &defs {
            if !name.starts_with("SIOC") || DEFERRED_PREFIXES.iter().any(|p| name.starts_with(p)) {
                continue;
            }
            let (mac, args) = text.split_once('(').expect(name);
            let args = args.trim_end().strip_suffix(')').expect(name);
            let parts: std::vec::Vec<&str> = args.split(',').map(str::trim).collect();
            let dir = match mac.trim() {
                "_IOR" => IOC_OUT,
                "_IOW" => IOC_IN,
                "_IOWR" => IOC_INOUT,
                m => panic!("{name}: {m}"),
            };
            let group = u64::from(parts[0].as_bytes()[1]);
            let num: u64 = parts[1].parse().expect(name);
            let len = c_size(parts[2]) as u64;
            let want = dir | ((len & IOCPARM_MASK) << 16) | (group << 8) | num;
            let ours = OURS.iter().find(|(n, _)| n == name);
            let Some((_, value)) = ours else {
                panic!("{name} is not ported");
            };
            assert_eq!(*value, want, "{name}");
            seen += 1;
        }
        assert_eq!(seen, OURS.len());
    }
}
/* </TESTS> */
