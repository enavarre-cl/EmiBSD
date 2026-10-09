/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1993
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
 *	@(#)if_ether.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! Ethernet protocol types: `<net/ethertypes.h>`.
//!
//! Upstream: sys/net/ethertypes.h @ 3ce1f3f79392
//!
//! According to "assigned numbers", the Ethernet protocol numbers are also used as ARP
//! protocol type numbers; they are factored out here to avoid pulling all the Ethernet header
//! file into the hardware independent ARP code. Values 0x0000-0x05DC (0..1500) are generally
//! IEEE 802.3 length fields, with some conflicts. The constants are host-order `u16`s;
//! `ether_type` on the wire is in network order (`htons(ETHERTYPE_IP)`).
//!
//! The C's comments listing unnamed ranges (`0x0400 Nixdorf`, `0x1000 - 0x100F Berkeley
//! Trailer`, ...) document no constant and are not repeated here.
//!
//! Status: `ported`.
//!
//! ## Deviations
//! - None.

/// IEEE 802.3 packet.
pub const ETHERTYPE_8023: u16 = 0x0004;
/// Xerox PUP protocol - see 0A00.
pub const ETHERTYPE_PUP: u16 = 0x0200;
/// PUP Address Translation - see 0A01.
pub const ETHERTYPE_PUPAT: u16 = 0x0200;
/// ???
pub const ETHERTYPE_SPRITE: u16 = 0x0500;
/// XNS.
pub const ETHERTYPE_NS: u16 = 0x0600;
/// XNS Address Translation (3Mb only)
pub const ETHERTYPE_NSAT: u16 = 0x0601;
/// DLOG (?)
pub const ETHERTYPE_DLOG1: u16 = 0x0660;
/// DLOG (?)
pub const ETHERTYPE_DLOG2: u16 = 0x0661;
/// IP protocol.
pub const ETHERTYPE_IP: u16 = 0x0800;
/// X.75 Internet.
pub const ETHERTYPE_X75: u16 = 0x0801;
/// NBS Internet.
pub const ETHERTYPE_NBS: u16 = 0x0802;
/// ECMA Internet.
pub const ETHERTYPE_ECMA: u16 = 0x0803;
/// CHAOSnet.
pub const ETHERTYPE_CHAOS: u16 = 0x0804;
/// X.25 Level 3.
pub const ETHERTYPE_X25: u16 = 0x0805;
/// Address resolution protocol.
pub const ETHERTYPE_ARP: u16 = 0x0806;
/// XNS Compatibility.
pub const ETHERTYPE_NSCOMPAT: u16 = 0x0807;
/// Frame Relay ARP (RFC1701)
pub const ETHERTYPE_FRARP: u16 = 0x0808;
/// Ungermann-Bass network debugger.
pub const ETHERTYPE_UBDEBUG: u16 = 0x0900;
/// Xerox IEEE802.3 PUP.
pub const ETHERTYPE_IEEEPUP: u16 = 0x0A00;
/// Xerox IEEE802.3 PUP Address Translation.
pub const ETHERTYPE_IEEEPUPAT: u16 = 0x0A01;
/// Banyan VINES.
pub const ETHERTYPE_VINES: u16 = 0x0BAD;
/// Banyan VINES Loopback.
pub const ETHERTYPE_VINESLOOP: u16 = 0x0BAE;
/// Banyan VINES Echo.
pub const ETHERTYPE_VINESECHO: u16 = 0x0BAF;
/// Trailer packet. The `ETHERTYPE_NTRAILER` packet types starting here have
/// (type - `ETHERTYPE_TRAIL`) * 512 bytes of data followed by an Ethernet type and then the
/// (variable-length) header.
pub const ETHERTYPE_TRAIL: u16 = 0x1000;
/// Number of trailer packet types.
pub const ETHERTYPE_NTRAILER: u16 = 16;
/// DCA - Multicast.
pub const ETHERTYPE_DCA: u16 = 0x1234;
/// VALID system protocol.
pub const ETHERTYPE_VALID: u16 = 0x1600;
/// Artificial Horizons ("Aviator" dogfight simulator \[on Sun\])
pub const ETHERTYPE_DOGFIGHT: u16 = 0x1989;
/// Datapoint Corporation (RCL lan protocol)
pub const ETHERTYPE_RCL: u16 = 0x1995;
/// NBMA Next Hop Resolution Protocol (RFC2332)
pub const ETHERTYPE_NHRP: u16 = 0x2001;
/// 3Com NBP virtual circuit datagram (like XNS SPP) not registered.
pub const ETHERTYPE_NBPVCD: u16 = 0x3C00;
/// 3Com NBP System control datagram not registered.
pub const ETHERTYPE_NBPSCD: u16 = 0x3C01;
/// 3Com NBP Connect request (virtual cct) not registered.
pub const ETHERTYPE_NBPCREQ: u16 = 0x3C02;
/// 3Com NBP Connect response not registered.
pub const ETHERTYPE_NBPCRSP: u16 = 0x3C03;
/// 3Com NBP Connect complete not registered.
pub const ETHERTYPE_NBPCC: u16 = 0x3C04;
/// 3Com NBP Close request (virtual cct) not registered.
pub const ETHERTYPE_NBPCLREQ: u16 = 0x3C05;
/// 3Com NBP Close response not registered.
pub const ETHERTYPE_NBPCLRSP: u16 = 0x3C06;
/// 3Com NBP Datagram (like XNS IDP) not registered.
pub const ETHERTYPE_NBPDG: u16 = 0x3C07;
/// 3Com NBP Datagram broadcast not registered.
pub const ETHERTYPE_NBPDGB: u16 = 0x3C08;
/// 3Com NBP Claim NetBIOS name not registered.
pub const ETHERTYPE_NBPCLAIM: u16 = 0x3C09;
/// 3Com NBP Delete Netbios name not registered.
pub const ETHERTYPE_NBPDLTE: u16 = 0x3C0A;
/// 3Com NBP Remote adaptor status request not registered.
pub const ETHERTYPE_NBPRAS: u16 = 0x3C0B;
/// 3Com NBP Remote adaptor response not registered.
pub const ETHERTYPE_NBPRAR: u16 = 0x3C0C;
/// 3Com NBP Reset not registered.
pub const ETHERTYPE_NBPRST: u16 = 0x3C0D;
/// PCS Basic Block Protocol.
pub const ETHERTYPE_PCS: u16 = 0x4242;
/// Information Modes Little Big LAN diagnostic.
pub const ETHERTYPE_IMLBLDIAG: u16 = 0x424C;
/// THD - Diddle.
pub const ETHERTYPE_DIDDLE: u16 = 0x4321;
/// Information Modes Little Big LAN.
pub const ETHERTYPE_IMLBL: u16 = 0x4C42;
/// BBN Simnet Private.
pub const ETHERTYPE_SIMNET: u16 = 0x5208;
/// DEC Unassigned, experimental.
pub const ETHERTYPE_DECEXPER: u16 = 0x6000;
/// DEC MOP dump/load.
pub const ETHERTYPE_MOPDL: u16 = 0x6001;
/// DEC MOP remote console.
pub const ETHERTYPE_MOPRC: u16 = 0x6002;
/// DEC DECNET Phase IV route.
#[allow(non_upper_case_globals)] // the C name
pub const ETHERTYPE_DECnet: u16 = 0x6003;
/// Libpcap, tcpdump.
pub const ETHERTYPE_DN: u16 = ETHERTYPE_DECnet;
/// DEC LAT.
pub const ETHERTYPE_LAT: u16 = 0x6004;
/// DEC diagnostic protocol (at interface initialization?)
pub const ETHERTYPE_DECDIAG: u16 = 0x6005;
/// DEC customer protocol.
pub const ETHERTYPE_DECCUST: u16 = 0x6006;
/// DEC LAVC, SCA.
pub const ETHERTYPE_SCA: u16 = 0x6007;
/// DEC AMBER.
pub const ETHERTYPE_AMBER: u16 = 0x6008;
/// DEC MUMPS.
pub const ETHERTYPE_DECMUMPS: u16 = 0x6009;
/// Trans Ether Bridging (RFC1701)
pub const ETHERTYPE_TRANSETHER: u16 = 0x6558;
/// Raw Frame Relay (RFC1701)
pub const ETHERTYPE_RAWFR: u16 = 0x6559;
/// Ungermann-Bass download.
pub const ETHERTYPE_UBDL: u16 = 0x7000;
/// Ungermann-Bass NIUs.
pub const ETHERTYPE_UBNIU: u16 = 0x7001;
/// Ungermann-Bass diagnostic/loopback.
pub const ETHERTYPE_UBDIAGLOOP: u16 = 0x7002;
/// Ungermann-Bass ??? (NMC to/from UB Bridge)
pub const ETHERTYPE_UBNMC: u16 = 0x7003;
/// Ungermann-Bass Bridge Spanning Tree.
pub const ETHERTYPE_UBBST: u16 = 0x7005;
/// OS/9 Microware.
pub const ETHERTYPE_OS9: u16 = 0x7007;
/// OS/9 Net?
pub const ETHERTYPE_OS9NET: u16 = 0x7009;
/// Racal-Interlan.
pub const ETHERTYPE_RACAL: u16 = 0x7030;
/// Prime NTS (Network Terminal Service)
pub const ETHERTYPE_PRIMENTS: u16 = 0x7031;
/// Cabletron.
pub const ETHERTYPE_CABLETRON: u16 = 0x7034;
/// Cronus VLN.
pub const ETHERTYPE_CRONUSVLN: u16 = 0x8003;
/// Cronus Direct.
pub const ETHERTYPE_CRONUS: u16 = 0x8004;
/// HP Probe.
pub const ETHERTYPE_HP: u16 = 0x8005;
/// Nestar.
pub const ETHERTYPE_NESTAR: u16 = 0x8006;
/// AT&T/Stanford (local use)
pub const ETHERTYPE_ATTSTANFORD: u16 = 0x8008;
/// Excelan.
pub const ETHERTYPE_EXCELAN: u16 = 0x8010;
/// SGI diagnostic type.
pub const ETHERTYPE_SG_DIAG: u16 = 0x8013;
/// SGI network games.
pub const ETHERTYPE_SG_NETGAMES: u16 = 0x8014;
/// SGI reserved type.
pub const ETHERTYPE_SG_RESV: u16 = 0x8015;
/// SGI bounce server.
pub const ETHERTYPE_SG_BOUNCE: u16 = 0x8016;
/// Apollo DOMAIN.
pub const ETHERTYPE_APOLLODOMAIN: u16 = 0x8019;
/// Tymeshare.
pub const ETHERTYPE_TYMSHARE: u16 = 0x802E;
/// Tigan, Inc.
pub const ETHERTYPE_TIGAN: u16 = 0x802F;
/// Reverse addr resolution protocol.
pub const ETHERTYPE_REVARP: u16 = 0x8035;
/// Aeonic Systems.
pub const ETHERTYPE_AEONIC: u16 = 0x8036;
/// IPX (Novell Netware?)
pub const ETHERTYPE_IPXNEW: u16 = 0x8037;
/// DEC LANBridge.
pub const ETHERTYPE_LANBRIDGE: u16 = 0x8038;
/// DEC DSM/DDP.
pub const ETHERTYPE_DSMD: u16 = 0x8039;
/// DEC Argonaut Console.
pub const ETHERTYPE_ARGONAUT: u16 = 0x803A;
/// DEC VAXELN.
pub const ETHERTYPE_VAXELN: u16 = 0x803B;
/// DEC DNS Naming Service.
pub const ETHERTYPE_DECDNS: u16 = 0x803C;
/// DEC Ethernet Encryption.
pub const ETHERTYPE_ENCRYPT: u16 = 0x803D;
/// DEC Distributed Time Service.
pub const ETHERTYPE_DECDTS: u16 = 0x803E;
/// DEC LAN Traffic Monitor.
pub const ETHERTYPE_DECLTM: u16 = 0x803F;
/// DEC PATHWORKS DECnet NETBIOS Emulation.
pub const ETHERTYPE_DECNETBIOS: u16 = 0x8040;
/// DEC Local Area System Transport.
pub const ETHERTYPE_DECLAST: u16 = 0x8041;
/// Planning Research Corp.
pub const ETHERTYPE_PLANNING: u16 = 0x8044;
/// DEC Availability Manager for Distributed Systems DECamds (but someone at DEC says not)
pub const ETHERTYPE_DECAM: u16 = 0x8048;
/// ExperData.
pub const ETHERTYPE_EXPERDATA: u16 = 0x8049;
/// Stanford V Kernel exp.
pub const ETHERTYPE_VEXP: u16 = 0x805B;
/// Stanford V Kernel prod.
pub const ETHERTYPE_VPROD: u16 = 0x805C;
/// Evans & Sutherland.
pub const ETHERTYPE_ES: u16 = 0x805D;
/// Little Machines.
pub const ETHERTYPE_LITTLE: u16 = 0x8060;
/// Counterpoint Computers.
pub const ETHERTYPE_COUNTERPOINT: u16 = 0x8062;
/// Veeco Integrated Auto.
pub const ETHERTYPE_VEECO: u16 = 0x8067;
/// General Dynamics.
pub const ETHERTYPE_GENDYN: u16 = 0x8068;
/// AT&T.
pub const ETHERTYPE_ATT: u16 = 0x8069;
/// Autophon.
pub const ETHERTYPE_AUTOPHON: u16 = 0x806A;
/// ComDesign.
pub const ETHERTYPE_COMDESIGN: u16 = 0x806C;
/// Compugraphic Corporation.
pub const ETHERTYPE_COMPUGRAPHIC: u16 = 0x806D;
/// Matra.
pub const ETHERTYPE_MATRA: u16 = 0x807A;
/// Dansk Data Elektronik.
pub const ETHERTYPE_DDE: u16 = 0x807B;
/// Merit Internodal (or Univ of Michigan?)
pub const ETHERTYPE_MERIT: u16 = 0x807C;
/// Vitalink TransLAN III Management.
pub const ETHERTYPE_VLTLMAN: u16 = 0x8080;
/// AppleTalk.
pub const ETHERTYPE_ATALK: u16 = 0x809B;
/// Old NetBSD.
pub const ETHERTYPE_AT: u16 = ETHERTYPE_ATALK;
/// HP-UX.
pub const ETHERTYPE_APPLETALK: u16 = ETHERTYPE_ATALK;
/// Spider Systems Ltd.
pub const ETHERTYPE_SPIDER: u16 = 0x809F;
/// Pacer Software.
pub const ETHERTYPE_PACER: u16 = 0x80C6;
/// Applitek Corporation.
pub const ETHERTYPE_APPLITEK: u16 = 0x80C7;
/// IBM SNA Services over Ethernet.
pub const ETHERTYPE_SNA: u16 = 0x80D5;
/// Varian Associates.
pub const ETHERTYPE_VARIAN: u16 = 0x80DD;
/// Retix.
pub const ETHERTYPE_RETIX: u16 = 0x80F2;
/// AppleTalk AARP.
pub const ETHERTYPE_AARP: u16 = 0x80F3;
/// Apollo Computer.
pub const ETHERTYPE_APOLLO: u16 = 0x80F7;
/// IEEE 802.1Q VLAN tagging (XXX conflicts)
pub const ETHERTYPE_VLAN: u16 = 0x8100;
/// Wellfleet; BOFL (Breath OF Life) pkts \[every 5-10 secs.\].
pub const ETHERTYPE_BOFL: u16 = 0x8102;
/// Wellfleet Communications.
pub const ETHERTYPE_WELLFLEET: u16 = 0x8103;
/// Talaris.
pub const ETHERTYPE_TALARIS: u16 = 0x812B;
/// Waterloo Microsystems Inc. (XXX which?)
pub const ETHERTYPE_WATERLOO: u16 = 0x8130;
/// Hayes Microcomputers (XXX which?)
pub const ETHERTYPE_HAYES: u16 = 0x8130;
/// VG Laboratory Systems.
pub const ETHERTYPE_VGLAB: u16 = 0x8131;
/// Novell (old) NetWare IPX (ECONFIG E option)
pub const ETHERTYPE_IPX: u16 = 0x8137;
/// Novell, Inc.
pub const ETHERTYPE_NOVELL: u16 = 0x8138;
/// M/MUMPS data sharing.
pub const ETHERTYPE_MUMPS: u16 = 0x813F;
/// Vrije Universiteit (NL) Amoeba 4 RPC (obsolete)
pub const ETHERTYPE_AMOEBA: u16 = 0x8145;
/// Vrije Universiteit (NL) FLIP (Fast Local Internet Protocol)
pub const ETHERTYPE_FLIP: u16 = 0x8146;
/// Vrije Universiteit (NL) \[reserved\].
pub const ETHERTYPE_VURESERVED: u16 = 0x8147;
/// Logicraft.
pub const ETHERTYPE_LOGICRAFT: u16 = 0x8148;
/// Network Computing Devices.
pub const ETHERTYPE_NCD: u16 = 0x8149;
/// Alpha Micro.
pub const ETHERTYPE_ALPHA: u16 = 0x814A;
/// SNMP over Ethernet (see RFC1089)
pub const ETHERTYPE_SNMP: u16 = 0x814C;
/// Technically Elite Concepts.
pub const ETHERTYPE_TEC: u16 = 0x814F;
/// Rational Corp.
pub const ETHERTYPE_RATIONAL: u16 = 0x8150;
/// Protocol Engines XTP.
pub const ETHERTYPE_XTP: u16 = 0x817D;
/// SGI/Time Warner prop.
pub const ETHERTYPE_SGITW: u16 = 0x817E;
/// HIPPI-FP encapsulation.
pub const ETHERTYPE_HIPPI_FP: u16 = 0x8180;
/// Scheduled Transfer STP, HIPPI-ST.
pub const ETHERTYPE_STP: u16 = 0x8181;
/// Motorola.
pub const ETHERTYPE_MOTOROLA: u16 = 0x818D;
/// PowerLAN NetBIOS/NetBEUI (PC)
pub const ETHERTYPE_NETBEUI: u16 = 0x8191;
/// Accton Technologies (unregistered)
pub const ETHERTYPE_ACCTON: u16 = 0x8390;
/// Talaris multicast.
pub const ETHERTYPE_TALARISMC: u16 = 0x852B;
/// Kalpana.
pub const ETHERTYPE_KALPANA: u16 = 0x8582;
/// SECTRA.
pub const ETHERTYPE_SECTRA: u16 = 0x86DB;
/// IP protocol version 6.
pub const ETHERTYPE_IPV6: u16 = 0x86DD;
/// Delta Controls.
pub const ETHERTYPE_DELTACON: u16 = 0x86DE;
/// ATOMIC.
pub const ETHERTYPE_ATOMIC: u16 = 0x86DF;
/// Control Technology Inc. RDP Without IP.
pub const ETHERTYPE_RDP: u16 = 0x8739;
/// Control Technology Inc. Mcast Industrial Ctrl Proto.
pub const ETHERTYPE_MICP: u16 = 0x873A;
/// TCP/IP Compression (RFC1701)
pub const ETHERTYPE_TCPCOMP: u16 = 0x876B;
/// IP Autonomous Systems (RFC1701)
pub const ETHERTYPE_IPAS: u16 = 0x876C;
/// Secure Data (RFC1701)
pub const ETHERTYPE_SECUREDATA: u16 = 0x876D;
/// 802.3x flow control packet.
pub const ETHERTYPE_FLOWCONTROL: u16 = 0x8808;
/// 803.3ad slow protocols (LACP/Marker)
pub const ETHERTYPE_SLOW: u16 = 0x8809;
/// PPP (obsolete by PPPOE)
pub const ETHERTYPE_PPP: u16 = 0x880B;
/// Hitachi Cable (Optoelectronic Systems Laboratory)
pub const ETHERTYPE_HITACHI: u16 = 0x8820;
/// MPLS Unicast.
pub const ETHERTYPE_MPLS: u16 = 0x8847;
/// MPLS Multicast.
pub const ETHERTYPE_MPLS_MCAST: u16 = 0x8848;
/// Axis Communications AB proprietary bootstrap/config.
pub const ETHERTYPE_AXIS: u16 = 0x8856;
/// PPP Over Ethernet Discovery Stage.
pub const ETHERTYPE_PPPOEDISC: u16 = 0x8863;
/// PPP Over Ethernet Session Stage.
pub const ETHERTYPE_PPPOE: u16 = 0x8864;
/// HP LanProbe test?
pub const ETHERTYPE_LANPROBE: u16 = 0x8888;
/// 802.1X EAP over LAN.
pub const ETHERTYPE_EAPOL: u16 = 0x888E;
/// ATA over Ethernet.
pub const ETHERTYPE_AOE: u16 = 0x88A2;
/// 802.1ad VLAN stacking.
pub const ETHERTYPE_QINQ: u16 = 0x88A8;
/// Link Layer Discovery Protocol.
pub const ETHERTYPE_LLDP: u16 = 0x88CC;
/// IEEE Std 802 - Local Experimental.
pub const ETHERTYPE_802_EX1: u16 = 0x88B5;
/// IEEE Std 802 - Local Experimental.
pub const ETHERTYPE_802_EX2: u16 = 0x88B6;
/// 802.1AE MACsec.
pub const ETHERTYPE_MACSEC: u16 = 0x88e5;
/// 802.1Q Provider Backbone Bridging.
pub const ETHERTYPE_PBB: u16 = 0x88e7;
/// IEEE 1588 Precision Time Protocol.
pub const ETHERTYPE_PTP: u16 = 0x88F7;
/// 802.1ag Connectivity Fault Management.
pub const ETHERTYPE_CFM: u16 = 0x8902;
/// Network Service Header (RFC8300)
pub const ETHERTYPE_NSH: u16 = 0x894F;
/// Loopback.
pub const ETHERTYPE_LOOPBACK: u16 = 0x9000;
/// DEC MOP loopback.
pub const ETHERTYPE_LBACK: u16 = ETHERTYPE_LOOPBACK;
/// 3Com (Formerly Bridge Communications), XNS Systems Management.
pub const ETHERTYPE_XNSSM: u16 = 0x9001;
/// 3Com (Formerly Bridge Communications), TCP/IP Systems Management.
pub const ETHERTYPE_TCPSM: u16 = 0x9002;
/// 3Com (Formerly Bridge Communications), loopback detection.
pub const ETHERTYPE_BCLOOP: u16 = 0x9003;
/// DECNET? Used by VAX 6220 DEBNI.
pub const ETHERTYPE_DEBNI: u16 = 0xAAAA;
/// Sonix Arpeggio.
pub const ETHERTYPE_SONIX: u16 = 0xFAF5;
/// BBN VITAL-LanBridge cache wakeups.
pub const ETHERTYPE_VITAL: u16 = 0xFF00;
/// Maximum valid ethernet type, reserved.
pub const ETHERTYPE_MAX: u16 = 0xFFFF;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/net/ethertypes.h");
        let ours = crate::reftest::assert_defines!(defs;
            ETHERTYPE_8023, ETHERTYPE_PUP, ETHERTYPE_PUPAT, ETHERTYPE_SPRITE, ETHERTYPE_NS,
            ETHERTYPE_NSAT, ETHERTYPE_DLOG1, ETHERTYPE_DLOG2, ETHERTYPE_IP, ETHERTYPE_X75,
            ETHERTYPE_NBS, ETHERTYPE_ECMA, ETHERTYPE_CHAOS, ETHERTYPE_X25, ETHERTYPE_ARP,
            ETHERTYPE_NSCOMPAT, ETHERTYPE_FRARP, ETHERTYPE_UBDEBUG, ETHERTYPE_IEEEPUP,
            ETHERTYPE_IEEEPUPAT, ETHERTYPE_VINES, ETHERTYPE_VINESLOOP, ETHERTYPE_VINESECHO,
            ETHERTYPE_TRAIL, ETHERTYPE_NTRAILER, ETHERTYPE_DCA, ETHERTYPE_VALID,
            ETHERTYPE_DOGFIGHT, ETHERTYPE_RCL, ETHERTYPE_NHRP, ETHERTYPE_NBPVCD, ETHERTYPE_NBPSCD,
            ETHERTYPE_NBPCREQ, ETHERTYPE_NBPCRSP, ETHERTYPE_NBPCC, ETHERTYPE_NBPCLREQ,
            ETHERTYPE_NBPCLRSP, ETHERTYPE_NBPDG, ETHERTYPE_NBPDGB, ETHERTYPE_NBPCLAIM,
            ETHERTYPE_NBPDLTE, ETHERTYPE_NBPRAS, ETHERTYPE_NBPRAR, ETHERTYPE_NBPRST,
            ETHERTYPE_PCS, ETHERTYPE_IMLBLDIAG, ETHERTYPE_DIDDLE, ETHERTYPE_IMLBL,
            ETHERTYPE_SIMNET, ETHERTYPE_DECEXPER, ETHERTYPE_MOPDL, ETHERTYPE_MOPRC,
            ETHERTYPE_DECnet, ETHERTYPE_DN, ETHERTYPE_LAT, ETHERTYPE_DECDIAG, ETHERTYPE_DECCUST,
            ETHERTYPE_SCA, ETHERTYPE_AMBER, ETHERTYPE_DECMUMPS, ETHERTYPE_TRANSETHER,
            ETHERTYPE_RAWFR, ETHERTYPE_UBDL, ETHERTYPE_UBNIU, ETHERTYPE_UBDIAGLOOP,
            ETHERTYPE_UBNMC, ETHERTYPE_UBBST, ETHERTYPE_OS9, ETHERTYPE_OS9NET, ETHERTYPE_RACAL,
            ETHERTYPE_PRIMENTS, ETHERTYPE_CABLETRON, ETHERTYPE_CRONUSVLN, ETHERTYPE_CRONUS,
            ETHERTYPE_HP, ETHERTYPE_NESTAR, ETHERTYPE_ATTSTANFORD, ETHERTYPE_EXCELAN,
            ETHERTYPE_SG_DIAG, ETHERTYPE_SG_NETGAMES, ETHERTYPE_SG_RESV, ETHERTYPE_SG_BOUNCE,
            ETHERTYPE_APOLLODOMAIN, ETHERTYPE_TYMSHARE, ETHERTYPE_TIGAN, ETHERTYPE_REVARP,
            ETHERTYPE_AEONIC, ETHERTYPE_IPXNEW, ETHERTYPE_LANBRIDGE, ETHERTYPE_DSMD,
            ETHERTYPE_ARGONAUT, ETHERTYPE_VAXELN, ETHERTYPE_DECDNS, ETHERTYPE_ENCRYPT,
            ETHERTYPE_DECDTS, ETHERTYPE_DECLTM, ETHERTYPE_DECNETBIOS, ETHERTYPE_DECLAST,
            ETHERTYPE_PLANNING, ETHERTYPE_DECAM, ETHERTYPE_EXPERDATA, ETHERTYPE_VEXP,
            ETHERTYPE_VPROD, ETHERTYPE_ES, ETHERTYPE_LITTLE, ETHERTYPE_COUNTERPOINT,
            ETHERTYPE_VEECO, ETHERTYPE_GENDYN, ETHERTYPE_ATT, ETHERTYPE_AUTOPHON,
            ETHERTYPE_COMDESIGN, ETHERTYPE_COMPUGRAPHIC, ETHERTYPE_MATRA, ETHERTYPE_DDE,
            ETHERTYPE_MERIT, ETHERTYPE_VLTLMAN, ETHERTYPE_ATALK, ETHERTYPE_AT,
            ETHERTYPE_APPLETALK, ETHERTYPE_SPIDER, ETHERTYPE_PACER, ETHERTYPE_APPLITEK,
            ETHERTYPE_SNA, ETHERTYPE_VARIAN, ETHERTYPE_RETIX, ETHERTYPE_AARP, ETHERTYPE_APOLLO,
            ETHERTYPE_VLAN, ETHERTYPE_BOFL, ETHERTYPE_WELLFLEET, ETHERTYPE_TALARIS,
            ETHERTYPE_WATERLOO, ETHERTYPE_HAYES, ETHERTYPE_VGLAB, ETHERTYPE_IPX, ETHERTYPE_NOVELL,
            ETHERTYPE_MUMPS, ETHERTYPE_AMOEBA, ETHERTYPE_FLIP, ETHERTYPE_VURESERVED,
            ETHERTYPE_LOGICRAFT, ETHERTYPE_NCD, ETHERTYPE_ALPHA, ETHERTYPE_SNMP, ETHERTYPE_TEC,
            ETHERTYPE_RATIONAL, ETHERTYPE_XTP, ETHERTYPE_SGITW, ETHERTYPE_HIPPI_FP, ETHERTYPE_STP,
            ETHERTYPE_MOTOROLA, ETHERTYPE_NETBEUI, ETHERTYPE_ACCTON, ETHERTYPE_TALARISMC,
            ETHERTYPE_KALPANA, ETHERTYPE_SECTRA, ETHERTYPE_IPV6, ETHERTYPE_DELTACON,
            ETHERTYPE_ATOMIC, ETHERTYPE_RDP, ETHERTYPE_MICP, ETHERTYPE_TCPCOMP, ETHERTYPE_IPAS,
            ETHERTYPE_SECUREDATA, ETHERTYPE_FLOWCONTROL, ETHERTYPE_SLOW, ETHERTYPE_PPP,
            ETHERTYPE_HITACHI, ETHERTYPE_MPLS, ETHERTYPE_MPLS_MCAST, ETHERTYPE_AXIS,
            ETHERTYPE_PPPOEDISC, ETHERTYPE_PPPOE, ETHERTYPE_LANPROBE, ETHERTYPE_EAPOL,
            ETHERTYPE_AOE, ETHERTYPE_QINQ, ETHERTYPE_LLDP, ETHERTYPE_802_EX1, ETHERTYPE_802_EX2,
            ETHERTYPE_MACSEC, ETHERTYPE_PBB, ETHERTYPE_PTP, ETHERTYPE_CFM, ETHERTYPE_NSH,
            ETHERTYPE_LOOPBACK, ETHERTYPE_LBACK, ETHERTYPE_XNSSM, ETHERTYPE_TCPSM,
            ETHERTYPE_BCLOOP, ETHERTYPE_DEBNI, ETHERTYPE_SONIX, ETHERTYPE_VITAL, ETHERTYPE_MAX,
        );
        crate::reftest::assert_complete(&defs, "ETHERTYPE_", &ours);
    }
}
/* </TESTS> */
