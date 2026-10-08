/*	$OpenBSD: if_types.h,v 1.25 2026/03/23 08:42:22 jsg Exp $	*/
/*	$NetBSD: if_types.h,v 1.17 2000/10/26 06:51:31 onoe Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1989, 1993, 1994
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
 *	@(#)if_types.h	8.3 (Berkeley) 4/28/95
 */
/* </LICENSES> */

/* <CODE> */
//! Interface types: `<net/if_types.h>`.
//!
//! Upstream: sys/net/if_types.h @ 3ce1f3f79392
//!
//! Interface types for benefit of parsing media address headers. This list is derived from
//! the SNMP list of ifTypes, originally documented in RFC 1573, now maintained as
//! <https://www.iana.org/assignments/ianaiftype-mib>. They are `u8`, the type of `ifi_type`.
//!
//! Status: `ported`.
//!
//! ## Deviations
//! - None.

/// None of the following.
pub const IFT_OTHER: u8 = 0x01;
/// Old-style arpanet imp.
pub const IFT_1822: u8 = 0x02;
/// HDH arpanet imp.
pub const IFT_HDH1822: u8 = 0x03;
/// X25 to imp.
pub const IFT_X25DDN: u8 = 0x04;
/// PDN X25 interface (RFC877)
pub const IFT_X25: u8 = 0x05;
/// Ethernet CSMA/CD.
pub const IFT_ETHER: u8 = 0x06;
/// CSMA/CD.
pub const IFT_ISO88023: u8 = 0x07;
/// Token Bus.
pub const IFT_ISO88024: u8 = 0x08;
/// Token Ring.
pub const IFT_ISO88025: u8 = 0x09;
/// MAN.
pub const IFT_ISO88026: u8 = 0x0a;
/// `IFT_STARLAN`.
pub const IFT_STARLAN: u8 = 0x0b;
/// Proteon 10MBit ring.
pub const IFT_P10: u8 = 0x0c;
/// Proteon 80MBit ring.
pub const IFT_P80: u8 = 0x0d;
/// Hyperchannel.
pub const IFT_HY: u8 = 0x0e;
/// `IFT_FDDI`.
pub const IFT_FDDI: u8 = 0x0f;
/// `IFT_LAPB`.
pub const IFT_LAPB: u8 = 0x10;
/// `IFT_SDLC`.
pub const IFT_SDLC: u8 = 0x11;
/// `IFT_T1`.
pub const IFT_T1: u8 = 0x12;
/// E1 - european T1.
pub const IFT_CEPT: u8 = 0x13;
/// `IFT_ISDNBASIC`.
pub const IFT_ISDNBASIC: u8 = 0x14;
/// `IFT_ISDNPRIMARY`.
pub const IFT_ISDNPRIMARY: u8 = 0x15;
/// Proprietary PTP serial.
pub const IFT_PTPSERIAL: u8 = 0x16;
/// RFC 1331.
pub const IFT_PPP: u8 = 0x17;
/// Loopback.
pub const IFT_LOOP: u8 = 0x18;
/// ISO over IP.
pub const IFT_EON: u8 = 0x19;
/// Obsolete 3MB experimental ethernet.
pub const IFT_XETHER: u8 = 0x1a;
/// XNS over IP.
pub const IFT_NSIP: u8 = 0x1b;
/// IP over generic TTY.
pub const IFT_SLIP: u8 = 0x1c;
/// Ultra Technologies.
pub const IFT_ULTRA: u8 = 0x1d;
/// Generic T3.
pub const IFT_DS3: u8 = 0x1e;
/// SMDS.
pub const IFT_SIP: u8 = 0x1f;
/// Frame Relay DTE only.
pub const IFT_FRELAY: u8 = 0x20;
/// `IFT_RS232`.
pub const IFT_RS232: u8 = 0x21;
/// Parallel-port.
pub const IFT_PARA: u8 = 0x22;
/// `IFT_ARCNET`.
pub const IFT_ARCNET: u8 = 0x23;
/// `IFT_ARCNETPLUS`.
pub const IFT_ARCNETPLUS: u8 = 0x24;
/// ATM cells.
pub const IFT_ATM: u8 = 0x25;
/// `IFT_MIOX25`.
pub const IFT_MIOX25: u8 = 0x26;
/// SONET or SDH.
pub const IFT_SONET: u8 = 0x27;
/// `IFT_X25PLE`.
pub const IFT_X25PLE: u8 = 0x28;
/// `IFT_ISO88022LLC`.
pub const IFT_ISO88022LLC: u8 = 0x29;
/// `IFT_LOCALTALK`.
pub const IFT_LOCALTALK: u8 = 0x2a;
/// `IFT_SMDSDXI`.
pub const IFT_SMDSDXI: u8 = 0x2b;
/// Frame Relay DCE.
pub const IFT_FRELAYDCE: u8 = 0x2c;
/// `IFT_V35`.
pub const IFT_V35: u8 = 0x2d;
/// `IFT_HSSI`.
pub const IFT_HSSI: u8 = 0x2e;
/// `IFT_HIPPI`.
pub const IFT_HIPPI: u8 = 0x2f;
/// Generic Modem.
pub const IFT_MODEM: u8 = 0x30;
/// AAL5 over ATM.
pub const IFT_AAL5: u8 = 0x31;
/// `IFT_SONETPATH`.
pub const IFT_SONETPATH: u8 = 0x32;
/// `IFT_SONETVT`.
pub const IFT_SONETVT: u8 = 0x33;
/// SMDS InterCarrier Interface.
pub const IFT_SMDSICIP: u8 = 0x34;
/// Proprietary Virtual/internal.
pub const IFT_PROPVIRTUAL: u8 = 0x35;
/// Proprietary Multiplexing.
pub const IFT_PROPMUX: u8 = 0x36;
/// 100BaseVG.
pub const IFT_IEEE80212: u8 = 0x37;
/// Fibre Channel.
pub const IFT_FIBRECHANNEL: u8 = 0x38;
/// HIPPI interfaces.
pub const IFT_HIPPIINTERFACE: u8 = 0x39;
/// Obsolete, use either 0x20 or 0x2c.
pub const IFT_FRAMERELAYINTERCONNECT: u8 = 0x3a;
/// ATM Emulated LAN for 802.3.
pub const IFT_AFLANE8023: u8 = 0x3b;
/// ATM Emulated LAN for 802.5.
pub const IFT_AFLANE8025: u8 = 0x3c;
/// ATM Emulated circuit.
pub const IFT_CCTEMUL: u8 = 0x3d;
/// Fast Ethernet (100BaseT)
pub const IFT_FASTETHER: u8 = 0x3e;
/// ISDN and X.25.
pub const IFT_ISDN: u8 = 0x3f;
/// CCITT V.11/X.21.
pub const IFT_V11: u8 = 0x40;
/// CCITT V.36.
pub const IFT_V36: u8 = 0x41;
/// CCITT G703 at 64Kbps.
pub const IFT_G703AT64K: u8 = 0x42;
/// Obsolete see DS1-MIB.
pub const IFT_G703AT2MB: u8 = 0x43;
/// SNA QLLC.
pub const IFT_QLLC: u8 = 0x44;
/// Fast Ethernet (100BaseFX)
pub const IFT_FASTETHERFX: u8 = 0x45;
/// Channel.
pub const IFT_CHANNEL: u8 = 0x46;
/// Radio spread spectrum.
pub const IFT_IEEE80211: u8 = 0x47;
/// IBM System 360/370 OEMI Channel.
pub const IFT_IBM370PARCHAN: u8 = 0x48;
/// IBM Enterprise Systems Connection.
pub const IFT_ESCON: u8 = 0x49;
/// Data Link Switching.
pub const IFT_DLSW: u8 = 0x4a;
/// ISDN S/T interface.
pub const IFT_ISDNS: u8 = 0x4b;
/// ISDN U interface.
pub const IFT_ISDNU: u8 = 0x4c;
/// Link Access Protocol D.
pub const IFT_LAPD: u8 = 0x4d;
/// IP Switching Objects.
pub const IFT_IPSWITCH: u8 = 0x4e;
/// Remote Source Route Bridging.
pub const IFT_RSRB: u8 = 0x4f;
/// ATM Logical Port.
pub const IFT_ATMLOGICAL: u8 = 0x50;
/// Digital Signal Level 0.
pub const IFT_DS0: u8 = 0x51;
/// Group of ds0s on the same ds1.
pub const IFT_DS0BUNDLE: u8 = 0x52;
/// Bisynchronous Protocol.
pub const IFT_BSC: u8 = 0x53;
/// Asynchronous Protocol.
pub const IFT_ASYNC: u8 = 0x54;
/// Combat Net Radio.
pub const IFT_CNR: u8 = 0x55;
/// ISO 802.5r DTR.
pub const IFT_ISO88025DTR: u8 = 0x56;
/// Ext Pos Loc Report Sys.
pub const IFT_EPLRS: u8 = 0x57;
/// Appletalk Remote Access Protocol.
pub const IFT_ARAP: u8 = 0x58;
/// Proprietary Connectionless Protocol.
pub const IFT_PROPCNLS: u8 = 0x59;
/// CCITT-ITU X.29 PAD Protocol.
pub const IFT_HOSTPAD: u8 = 0x5a;
/// CCITT-ITU X.3 PAD Facility.
pub const IFT_TERMPAD: u8 = 0x5b;
/// Multiproto Interconnect over FR.
pub const IFT_FRAMERELAYMPI: u8 = 0x5c;
/// CCITT-ITU X213.
pub const IFT_X213: u8 = 0x5d;
/// Asymmetric Digital Subscriber Loop.
pub const IFT_ADSL: u8 = 0x5e;
/// Rate-Adapt. Digital Subscriber Loop.
pub const IFT_RADSL: u8 = 0x5f;
/// Symmetric Digital Subscriber Loop.
pub const IFT_SDSL: u8 = 0x60;
/// Very H-Speed Digital Subscrib. Loop.
pub const IFT_VDSL: u8 = 0x61;
/// ISO 802.5 CRFP.
pub const IFT_ISO88025CRFPINT: u8 = 0x62;
/// Myricom Myrinet.
pub const IFT_MYRINET: u8 = 0x63;
/// Voice recEive and transMit.
pub const IFT_VOICEEM: u8 = 0x64;
/// Voice Foreign Exchange Office.
pub const IFT_VOICEFXO: u8 = 0x65;
/// Voice Foreign Exchange Station.
pub const IFT_VOICEFXS: u8 = 0x66;
/// Voice encapsulation.
pub const IFT_VOICEENCAP: u8 = 0x67;
/// Voice over IP encapsulation.
pub const IFT_VOICEOVERIP: u8 = 0x68;
/// ATM DXI.
pub const IFT_ATMDXI: u8 = 0x69;
/// ATM FUNI.
pub const IFT_ATMFUNI: u8 = 0x6a;
/// ATM IMA.
pub const IFT_ATMIMA: u8 = 0x6b;
/// PPP Multilink Bundle.
pub const IFT_PPPMULTILINKBUNDLE: u8 = 0x6c;
/// IBM ipOverCdlc.
pub const IFT_IPOVERCDLC: u8 = 0x6d;
/// IBM Common Link Access to Workstn.
pub const IFT_IPOVERCLAW: u8 = 0x6e;
/// IBM stackToStack.
pub const IFT_STACKTOSTACK: u8 = 0x6f;
/// IBM VIPA.
pub const IFT_VIRTUALIPADDRESS: u8 = 0x70;
/// IBM multi-protocol channel support.
pub const IFT_MPC: u8 = 0x71;
/// IBM ipOverAtm.
pub const IFT_IPOVERATM: u8 = 0x72;
/// ISO 802.5j Fiber Token Ring.
pub const IFT_ISO88025FIBER: u8 = 0x73;
/// IBM twinaxial data link control.
pub const IFT_TDLC: u8 = 0x74;
/// Gigabit Ethernet.
pub const IFT_GIGABITETHERNET: u8 = 0x75;
/// HDLC.
pub const IFT_HDLC: u8 = 0x76;
/// LAP F.
pub const IFT_LAPF: u8 = 0x77;
/// V.37.
pub const IFT_V37: u8 = 0x78;
/// Multi-Link Protocol.
pub const IFT_X25MLP: u8 = 0x79;
/// X25 Hunt Group.
pub const IFT_X25HUNTGROUP: u8 = 0x7a;
/// Transp HDLC.
pub const IFT_TRANSPHDLC: u8 = 0x7b;
/// Interleave channel.
pub const IFT_INTERLEAVE: u8 = 0x7c;
/// Fast channel.
pub const IFT_FAST: u8 = 0x7d;
/// IP (for APPN HPR in IP networks)
pub const IFT_IP: u8 = 0x7e;
/// CATV Mac Layer.
pub const IFT_DOCSCABLEMACLAYER: u8 = 0x7f;
/// CATV Downstream interface.
pub const IFT_DOCSCABLEDOWNSTREAM: u8 = 0x80;
/// CATV Upstream interface.
pub const IFT_DOCSCABLEUPSTREAM: u8 = 0x81;
/// Avalon Parallel Processor.
pub const IFT_A12MPPSWITCH: u8 = 0x82;
/// Encapsulation interface.
pub const IFT_TUNNEL: u8 = 0x83;
/// Coffee pot.
pub const IFT_COFFEE: u8 = 0x84;
/// Circuit Emulation Service.
pub const IFT_CES: u8 = 0x85;
/// (x)  ATM Sub Interface.
pub const IFT_ATMSUBINTERFACE: u8 = 0x86;
/// Layer 2 Virtual LAN using 802.1Q.
pub const IFT_L2VLAN: u8 = 0x87;
/// Layer 3 Virtual LAN - IP Protocol.
pub const IFT_L3IPVLAN: u8 = 0x88;
/// Layer 3 Virtual LAN - IPX Prot.
pub const IFT_L3IPXVLAN: u8 = 0x89;
/// IP over Power Lines.
pub const IFT_DIGITALPOWERLINE: u8 = 0x8a;
/// (xxx)  Multimedia Mail over IP.
pub const IFT_MEDIAMAILOVERIP: u8 = 0x8b;
/// Dynamic synchronous Transfer Mode.
pub const IFT_DTM: u8 = 0x8c;
/// Data Communications Network.
pub const IFT_DCN: u8 = 0x8d;
/// IP Forwarding Interface.
pub const IFT_IPFORWARD: u8 = 0x8e;
/// Multi-rate Symmetric DSL.
pub const IFT_MSDSL: u8 = 0x8f;
/// IEEE1394 High Performance SerialBus.
pub const IFT_IEEE1394: u8 = 0x90;
/// HIPPI-6400.
pub const IFT_IFGSN: u8 = 0x91;
/// DVB-RCC MAC Layer.
pub const IFT_DVBRCCMACLAYER: u8 = 0x92;
/// DVB-RCC Downstream Channel.
pub const IFT_DVBRCCDOWNSTREAM: u8 = 0x93;
/// DVB-RCC Upstream Channel.
pub const IFT_DVBRCCUPSTREAM: u8 = 0x94;
/// ATM Virtual Interface.
pub const IFT_ATMVIRTUAL: u8 = 0x95;
/// MPLS Tunnel Virtual Interface.
pub const IFT_MPLSTUNNEL: u8 = 0x96;
/// Spatial Reuse Protocol.
pub const IFT_SRP: u8 = 0x97;
/// Voice over ATM.
pub const IFT_VOICEOVERATM: u8 = 0x98;
/// Voice Over Frame Relay.
pub const IFT_VOICEOVERFRAMERELAY: u8 = 0x99;
/// Digital Subscriber Loop over ISDN.
pub const IFT_IDSL: u8 = 0x9a;
/// Avici Composite Link Interface.
pub const IFT_COMPOSITELINK: u8 = 0x9b;
/// SS7 Signaling Link.
pub const IFT_SS7SIGLINK: u8 = 0x9c;
/// Prop. P2P wireless interface.
pub const IFT_PROPWIRELESSP2P: u8 = 0x9d;
/// Frame forward Interface.
pub const IFT_FRFORWARD: u8 = 0x9e;
/// Multiprotocol over ATM AAL5.
pub const IFT_RFC1483: u8 = 0x9f;
/// USB Interface.
pub const IFT_USB: u8 = 0xa0;
/// IEEE 802.3ad Link Aggregate.
pub const IFT_IEEE8023ADLAG: u8 = 0xa1;
/// BGP Policy Accounting.
pub const IFT_BGPPOLICYACCOUNTING: u8 = 0xa2;
/// FRF.16 Multilink Frame Relay.
pub const IFT_FRF16MFRBUNDLE: u8 = 0xa3;
/// H323 Gatekeeper.
pub const IFT_H323GATEKEEPER: u8 = 0xa4;
/// H323 Voice and Video Proxy.
pub const IFT_H323PROXY: u8 = 0xa5;
/// MPLS.
pub const IFT_MPLS: u8 = 0xa6;
/// Multi-frequency signaling link.
pub const IFT_MFSIGLINK: u8 = 0xa7;
/// High Bit-Rate DSL, 2nd gen.
pub const IFT_HDSL2: u8 = 0xa8;
/// Multirate HDSL2.
pub const IFT_SHDSL: u8 = 0xa9;
/// Facility Data Link (4Kbps) on a DS1.
pub const IFT_DS1FDL: u8 = 0xaa;
/// Packet over SONET/SDH Interface.
pub const IFT_POS: u8 = 0xab;
/// DVB-ASI Input.
pub const IFT_DVBASILN: u8 = 0xac;
/// DVB-ASI Output.
pub const IFT_DVBASIOUT: u8 = 0xad;
/// Power Line Communications.
pub const IFT_PLC: u8 = 0xae;
/// Non-Facility Associated Signaling.
pub const IFT_NFAS: u8 = 0xaf;
/// TROO8.
pub const IFT_TR008: u8 = 0xb0;
/// Remote Digital Terminal.
pub const IFT_GR303RDT: u8 = 0xb1;
/// Integrated Digital Terminal.
pub const IFT_GR303IDT: u8 = 0xb2;
/// ISUP.
pub const IFT_ISUP: u8 = 0xb3;
/// Prop/Wireless MAC Layer.
pub const IFT_PROPDOCSWIRELESSMACLAYER: u8 = 0xb4;
/// Prop/Wireless Downstream.
pub const IFT_PROPDOCSWIRELESSDOWNSTREAM: u8 = 0xb5;
/// Prop/Wireless Upstream.
pub const IFT_PROPDOCSWIRELESSUPSTREAM: u8 = 0xb6;
/// HIPERLAN Type 2 Radio Interface.
pub const IFT_HIPERLAN2: u8 = 0xb7;
/// PropBroadbandWirelessAccess P2MP.
pub const IFT_PROPBWAP2MP: u8 = 0xb8;
/// SONET Overhead Channel.
pub const IFT_SONETOVERHEADCHANNEL: u8 = 0xb9;
/// Digital Wrapper Overhead.
pub const IFT_DIGITALWRAPPEROVERHEADCHANNEL: u8 = 0xba;
/// ATM adaptation layer 2.
pub const IFT_AAL2: u8 = 0xbb;
/// MAC layer over radio links.
pub const IFT_RADIOMAC: u8 = 0xbc;
/// ATM over radio links.
pub const IFT_ATMRADIO: u8 = 0xbd;
/// Inter-Machine Trunks.
pub const IFT_IMT: u8 = 0xbe;
/// Multiple Virtual Lines DSL.
pub const IFT_MVL: u8 = 0xbf;
/// Long Reach DSL.
pub const IFT_REACHDSL: u8 = 0xc0;
/// Frame Relay DLCI End Point.
pub const IFT_FRDLCIENDPT: u8 = 0xc1;
/// ATM VCI End Point.
pub const IFT_ATMVCIENDPT: u8 = 0xc2;
/// Optical Channel.
pub const IFT_OPTICALCHANNEL: u8 = 0xc3;
/// Optical Transport.
pub const IFT_OPTICALTRANSPORT: u8 = 0xc4;
/// Proprietary ATM.
pub const IFT_PROPATM: u8 = 0xc5;
/// Voice Over Cable Interface.
pub const IFT_VOICEOVERCABLE: u8 = 0xc6;
/// Infiniband.
pub const IFT_INFINIBAND: u8 = 0xc7;
/// TE Link.
pub const IFT_TELINK: u8 = 0xc8;
/// Q.2931.
pub const IFT_Q2931: u8 = 0xc9;
/// Virtual Trunk Group.
pub const IFT_VIRTUALTG: u8 = 0xca;
/// SIP Trunk Group.
pub const IFT_SIPTG: u8 = 0xcb;
/// SIP Signaling.
pub const IFT_SIPSIG: u8 = 0xcc;
/// CATV Upstream Channel.
pub const IFT_DOCSCABLEUPSTREAMCHANNEL: u8 = 0xcd;
/// Acorn Econet.
pub const IFT_ECONET: u8 = 0xce;
/// FSAN 155Mb Symmetrical PON interface.
pub const IFT_PON155: u8 = 0xcf;
/// FSAN 622Mb Symmetrical PON interface.
pub const IFT_PON622: u8 = 0xd0;
/// Transparent bridge interface.
pub const IFT_BRIDGE: u8 = 0xd1;
/// Interface common to multiple lines.
pub const IFT_LINEGROUP: u8 = 0xd2;
/// Voice E&M Feature Group D.
pub const IFT_VOICEEMFGD: u8 = 0xd3;
/// Voice FGD Exchange Access North American.
pub const IFT_VOICEFGDEANA: u8 = 0xd4;
/// Voice Direct Inward Dialing.
pub const IFT_VOICEDID: u8 = 0xd5;
/// `IFT_GIF`.
pub const IFT_GIF: u8 = 0xf0;
/// `IFT_DUMMY`.
pub const IFT_DUMMY: u8 = 0xf1;
/// `IFT_PVC`.
pub const IFT_PVC: u8 = 0xf2;
/// `IFT_FAITH`.
pub const IFT_FAITH: u8 = 0xf3;
/// Encapsulation.
pub const IFT_ENC: u8 = 0xf4;
/// Packet filter logging.
pub const IFT_PFLOG: u8 = 0xf5;
/// Packet filter state syncing.
pub const IFT_PFSYNC: u8 = 0xf6;
/// Common Address Redundancy Protocol.
pub const IFT_CARP: u8 = 0xf7;
/// Bluetooth.
pub const IFT_BLUETOOTH: u8 = 0xf8;
/// Pflow.
pub const IFT_PFLOW: u8 = 0xf9;
/// Mobile Broadband Interface Model.
pub const IFT_MBIM: u8 = 0xfa;
/// WireGuard tunnel.
pub const IFT_WIREGUARD: u8 = 0xfb;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/net/if_types.h");
        let ours = crate::reftest::assert_defines!(defs;
            IFT_OTHER, IFT_1822, IFT_HDH1822, IFT_X25DDN, IFT_X25, IFT_ETHER, IFT_ISO88023,
            IFT_ISO88024, IFT_ISO88025, IFT_ISO88026, IFT_STARLAN, IFT_P10, IFT_P80, IFT_HY,
            IFT_FDDI, IFT_LAPB, IFT_SDLC, IFT_T1, IFT_CEPT, IFT_ISDNBASIC, IFT_ISDNPRIMARY,
            IFT_PTPSERIAL, IFT_PPP, IFT_LOOP, IFT_EON, IFT_XETHER, IFT_NSIP, IFT_SLIP, IFT_ULTRA,
            IFT_DS3, IFT_SIP, IFT_FRELAY, IFT_RS232, IFT_PARA, IFT_ARCNET, IFT_ARCNETPLUS,
            IFT_ATM, IFT_MIOX25, IFT_SONET, IFT_X25PLE, IFT_ISO88022LLC, IFT_LOCALTALK,
            IFT_SMDSDXI, IFT_FRELAYDCE, IFT_V35, IFT_HSSI, IFT_HIPPI, IFT_MODEM, IFT_AAL5,
            IFT_SONETPATH, IFT_SONETVT, IFT_SMDSICIP, IFT_PROPVIRTUAL, IFT_PROPMUX, IFT_IEEE80212,
            IFT_FIBRECHANNEL, IFT_HIPPIINTERFACE, IFT_FRAMERELAYINTERCONNECT, IFT_AFLANE8023,
            IFT_AFLANE8025, IFT_CCTEMUL, IFT_FASTETHER, IFT_ISDN, IFT_V11, IFT_V36, IFT_G703AT64K,
            IFT_G703AT2MB, IFT_QLLC, IFT_FASTETHERFX, IFT_CHANNEL, IFT_IEEE80211,
            IFT_IBM370PARCHAN, IFT_ESCON, IFT_DLSW, IFT_ISDNS, IFT_ISDNU, IFT_LAPD, IFT_IPSWITCH,
            IFT_RSRB, IFT_ATMLOGICAL, IFT_DS0, IFT_DS0BUNDLE, IFT_BSC, IFT_ASYNC, IFT_CNR,
            IFT_ISO88025DTR, IFT_EPLRS, IFT_ARAP, IFT_PROPCNLS, IFT_HOSTPAD, IFT_TERMPAD,
            IFT_FRAMERELAYMPI, IFT_X213, IFT_ADSL, IFT_RADSL, IFT_SDSL, IFT_VDSL,
            IFT_ISO88025CRFPINT, IFT_MYRINET, IFT_VOICEEM, IFT_VOICEFXO, IFT_VOICEFXS,
            IFT_VOICEENCAP, IFT_VOICEOVERIP, IFT_ATMDXI, IFT_ATMFUNI, IFT_ATMIMA,
            IFT_PPPMULTILINKBUNDLE, IFT_IPOVERCDLC, IFT_IPOVERCLAW, IFT_STACKTOSTACK,
            IFT_VIRTUALIPADDRESS, IFT_MPC, IFT_IPOVERATM, IFT_ISO88025FIBER, IFT_TDLC,
            IFT_GIGABITETHERNET, IFT_HDLC, IFT_LAPF, IFT_V37, IFT_X25MLP, IFT_X25HUNTGROUP,
            IFT_TRANSPHDLC, IFT_INTERLEAVE, IFT_FAST, IFT_IP, IFT_DOCSCABLEMACLAYER,
            IFT_DOCSCABLEDOWNSTREAM, IFT_DOCSCABLEUPSTREAM, IFT_A12MPPSWITCH, IFT_TUNNEL,
            IFT_COFFEE, IFT_CES, IFT_ATMSUBINTERFACE, IFT_L2VLAN, IFT_L3IPVLAN, IFT_L3IPXVLAN,
            IFT_DIGITALPOWERLINE, IFT_MEDIAMAILOVERIP, IFT_DTM, IFT_DCN, IFT_IPFORWARD, IFT_MSDSL,
            IFT_IEEE1394, IFT_IFGSN, IFT_DVBRCCMACLAYER, IFT_DVBRCCDOWNSTREAM, IFT_DVBRCCUPSTREAM,
            IFT_ATMVIRTUAL, IFT_MPLSTUNNEL, IFT_SRP, IFT_VOICEOVERATM, IFT_VOICEOVERFRAMERELAY,
            IFT_IDSL, IFT_COMPOSITELINK, IFT_SS7SIGLINK, IFT_PROPWIRELESSP2P, IFT_FRFORWARD,
            IFT_RFC1483, IFT_USB, IFT_IEEE8023ADLAG, IFT_BGPPOLICYACCOUNTING, IFT_FRF16MFRBUNDLE,
            IFT_H323GATEKEEPER, IFT_H323PROXY, IFT_MPLS, IFT_MFSIGLINK, IFT_HDSL2, IFT_SHDSL,
            IFT_DS1FDL, IFT_POS, IFT_DVBASILN, IFT_DVBASIOUT, IFT_PLC, IFT_NFAS, IFT_TR008,
            IFT_GR303RDT, IFT_GR303IDT, IFT_ISUP, IFT_PROPDOCSWIRELESSMACLAYER,
            IFT_PROPDOCSWIRELESSDOWNSTREAM, IFT_PROPDOCSWIRELESSUPSTREAM, IFT_HIPERLAN2,
            IFT_PROPBWAP2MP, IFT_SONETOVERHEADCHANNEL, IFT_DIGITALWRAPPEROVERHEADCHANNEL,
            IFT_AAL2, IFT_RADIOMAC, IFT_ATMRADIO, IFT_IMT, IFT_MVL, IFT_REACHDSL, IFT_FRDLCIENDPT,
            IFT_ATMVCIENDPT, IFT_OPTICALCHANNEL, IFT_OPTICALTRANSPORT, IFT_PROPATM,
            IFT_VOICEOVERCABLE, IFT_INFINIBAND, IFT_TELINK, IFT_Q2931, IFT_VIRTUALTG, IFT_SIPTG,
            IFT_SIPSIG, IFT_DOCSCABLEUPSTREAMCHANNEL, IFT_ECONET, IFT_PON155, IFT_PON622,
            IFT_BRIDGE, IFT_LINEGROUP, IFT_VOICEEMFGD, IFT_VOICEFGDEANA, IFT_VOICEDID, IFT_GIF,
            IFT_DUMMY, IFT_PVC, IFT_FAITH, IFT_ENC, IFT_PFLOG, IFT_PFSYNC, IFT_CARP,
            IFT_BLUETOOTH, IFT_PFLOW, IFT_MBIM, IFT_WIREGUARD,
        );
        crate::reftest::assert_complete(&defs, "IFT_", &ours);
    }
}
/* </TESTS> */
