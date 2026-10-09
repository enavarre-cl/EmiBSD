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

/*-
 * Copyright (c) 2014 Martin Pieuchot. All rights reserved.
 * Copyright (c) 2010 Hans Petter Selasky. All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! The xHCI registers and the structures the controller shares with the driver in memory:
//! `<dev/usb/xhcireg.h>`.
//!
//! Upstream: sys/dev/usb/xhcireg.h @ 3ce1f3f79392
//!
//! The capability registers sit at the start of the memory BAR; the operational registers
//! follow at `XHCI_CAPLENGTH`, the runtime registers at `XHCI_RTSOFF` and the doorbells at
//! `XHCI_DBOFF`. The contexts (`struct xhci_sctx`, `xhci_epctx`, `xhci_inctx`), the TRBs
//! (`struct xhci_trb`) and the event ring segment table (`struct xhci_erseg`) live in DMA
//! memory in the controller's little-endian layout.
//!
//! ## Deviations
//! - Register offsets are `usize` (`bus_size_t`); register bits, TRB fields and codes `u32`;
//!   the PCI configuration register numbers `i32` (`pci_conf_read`'s `int reg`).
//! - Function-like macros (`XHCI_PORTSC(n)`, `XHCI_TRB_GET_SLOT(x)`, ...) are `const fn`s
//!   with the macro's name in lower case (`docs/C_TO_RUST.md`).
//! - The structures are `#[repr(C)]` with their C sizes pinned by compile-time checks; the
//!   driver never takes a reference into DMA memory but reads and writes them through raw
//!   pointers (`xhci.rs`).
//! - `XHCI_TRB_FLAGS_BITMASK` is a byte string for `%b`, as the C's string literal.

use crate::machine::bus::BusSize;

/* Data Structure Boundary and Alignment Requirement. */
/// `XHCI_DCBAA_ALIGN`.
pub const XHCI_DCBAA_ALIGN: BusSize = 64;
/// `XHCI_ICTX_ALIGN`.
pub const XHCI_ICTX_ALIGN: BusSize = 64;
/// `XHCI_SCTX_ALIGN`.
pub const XHCI_SCTX_ALIGN: BusSize = 32;
/// `XHCI_OCTX_ALIGN`.
pub const XHCI_OCTX_ALIGN: BusSize = 32;
/// `XHCI_XFER_RING_ALIGN`.
pub const XHCI_XFER_RING_ALIGN: BusSize = 16;
/// `XHCI_CMDS_RING_ALIGN`.
pub const XHCI_CMDS_RING_ALIGN: BusSize = 64;
/// `XHCI_EVTS_RING_ALIGN`.
pub const XHCI_EVTS_RING_ALIGN: BusSize = 64;
/// `XHCI_RING_BOUNDARY`.
pub const XHCI_RING_BOUNDARY: BusSize = 64 * 1024;
/// `XHCI_ERST_ALIGN`.
pub const XHCI_ERST_ALIGN: BusSize = 64;
/// `XHCI_ERST_BOUNDARY`.
pub const XHCI_ERST_BOUNDARY: BusSize = 0;
/// `XHCI_SPAD_TABLE_ALIGN`.
pub const XHCI_SPAD_TABLE_ALIGN: BusSize = 64;

/* XHCI PCI config registers */
/// `PCI_CBMEM`: configuration base MEM.
pub const PCI_CBMEM: i32 = 0x10;

/// `PCI_INTERFACE_XHCI`.
pub const PCI_INTERFACE_XHCI: u32 = 0x30;

/// `PCI_USBREV`: RO USB protocol revision.
pub const PCI_USBREV: i32 = 0x60;
/// `PCI_USBREV_MASK`.
pub const PCI_USBREV_MASK: u32 = 0xff;
/// `PCI_USBREV_3_0`: USB 3.0.
pub const PCI_USBREV_3_0: u32 = 0x30;

/// `PCI_XHCI_FLADJ`: RW frame length adjust.
pub const PCI_XHCI_FLADJ: i32 = 0x61;

/// `PCI_XHCI_INTEL_XUSB2PR`: Intel USB2 Port Routing.
pub const PCI_XHCI_INTEL_XUSB2PR: i32 = 0xd0;
/// `PCI_XHCI_INTEL_XUSB2PRM`: Intel USB2 Port Routing Mask.
pub const PCI_XHCI_INTEL_XUSB2PRM: i32 = 0xd4;
/// `PCI_XHCI_INTEL_USB3_PSSEN`: Intel USB3 Port SuperSpeed Enable.
pub const PCI_XHCI_INTEL_USB3_PSSEN: i32 = 0xd8;
/// `PCI_XHCI_INTEL_USB3PRM`: Intel USB3 Port Routing Mask.
pub const PCI_XHCI_INTEL_USB3PRM: i32 = 0xdc;

/* XHCI capability registers */
/// `XHCI_CAPLENGTH`: RO Capability reg. length field.
pub const XHCI_CAPLENGTH: BusSize = 0x00;
/// `XHCI_RESERVED`: Reserved.
pub const XHCI_RESERVED: BusSize = 0x01;
/// `XHCI_HCIVERSION`: RO Interface version number.
pub const XHCI_HCIVERSION: BusSize = 0x02;
/// `XHCI_HCIVERSION_0_9`: xHCI version 0.9.
pub const XHCI_HCIVERSION_0_9: u32 = 0x0090;
/// `XHCI_HCIVERSION_1_0`: xHCI version 1.0.
pub const XHCI_HCIVERSION_1_0: u32 = 0x0100;

/// `XHCI_HCSPARAMS1`: RO structural parameters 1.
pub const XHCI_HCSPARAMS1: BusSize = 0x04;
/// `XHCI_HCS1_DEVSLOT_MAX(x)`.
pub const fn xhci_hcs1_devslot_max(x: u32) -> u32 {
    x & 0xff
}
/// `XHCI_HCS1_IRQ_MAX(x)`.
pub const fn xhci_hcs1_irq_max(x: u32) -> u32 {
    (x >> 8) & 0x3ff
}
/// `XHCI_HCS1_N_PORTS(x)`.
pub const fn xhci_hcs1_n_ports(x: u32) -> u32 {
    (x >> 24) & 0xff
}

/// `XHCI_HCSPARAMS2`: RO structural parameters 2.
pub const XHCI_HCSPARAMS2: BusSize = 0x08;
/// `XHCI_HCS2_IST(x)`.
pub const fn xhci_hcs2_ist(x: u32) -> u32 {
    x & 0x7
}
/// `XHCI_HCS2_IST_MICRO(x)`.
pub const fn xhci_hcs2_ist_micro(x: u32) -> bool {
    x & 0x8 == 0
}
/// `XHCI_HCS2_ERST_MAX(x)`.
pub const fn xhci_hcs2_erst_max(x: u32) -> u32 {
    (x >> 4) & 0xf
}
/// `XHCI_HCS2_ETE(x)`.
pub const fn xhci_hcs2_ete(x: u32) -> u32 {
    (x >> 8) & 0x1
}
/// `XHCI_HCS2_SPR(x)`.
pub const fn xhci_hcs2_spr(x: u32) -> u32 {
    (x >> 26) & 0x1
}
/// `XHCI_HCS2_SPB_MAX(x)`: the number of scratchpad buffers (hi and lo fields).
pub const fn xhci_hcs2_spb_max(x: u32) -> u32 {
    ((x >> 16) & 0x3e0) | ((x >> 27) & 0x1f)
}

/// `XHCI_HCSPARAMS3`: RO structural parameters 3.
pub const XHCI_HCSPARAMS3: BusSize = 0x0c;
/// `XHCI_HCS3_U1_DEL(x)`.
pub const fn xhci_hcs3_u1_del(x: u32) -> u32 {
    x & 0xff
}
/// `XHCI_HCS3_U2_DEL(x)`.
pub const fn xhci_hcs3_u2_del(x: u32) -> u32 {
    (x >> 16) & 0xffff
}

/// `XHCI_HCCPARAMS`: RO capability parameters.
pub const XHCI_HCCPARAMS: BusSize = 0x10;
/// `XHCI_HCC_AC64(x)`: 64-bit capable.
pub const fn xhci_hcc_ac64(x: u32) -> u32 {
    x & 0x1
}
/// `XHCI_HCC_BNC(x)`: BW negotiation.
pub const fn xhci_hcc_bnc(x: u32) -> u32 {
    (x >> 1) & 0x1
}
/// `XHCI_HCC_CSZ(x)`: Context size.
pub const fn xhci_hcc_csz(x: u32) -> u32 {
    (x >> 2) & 0x1
}
/// `XHCI_HCC_PPC(x)`: Port power control.
pub const fn xhci_hcc_ppc(x: u32) -> u32 {
    (x >> 3) & 0x1
}
/// `XHCI_HCC_PIND(x)`: Port indicators.
pub const fn xhci_hcc_pind(x: u32) -> u32 {
    (x >> 4) & 0x1
}
/// `XHCI_HCC_LHRC(x)`: Light HC reset.
pub const fn xhci_hcc_lhrc(x: u32) -> u32 {
    (x >> 5) & 0x1
}
/// `XHCI_HCC_LTC(x)`: Latency tolerance msg.
pub const fn xhci_hcc_ltc(x: u32) -> u32 {
    (x >> 6) & 0x1
}
/// `XHCI_HCC_NSS(x)`: No secondary sid.
pub const fn xhci_hcc_nss(x: u32) -> u32 {
    (x >> 7) & 0x1
}
/// `XHCI_HCC_PAE(x)`: Parse All Event Data.
pub const fn xhci_hcc_pae(x: u32) -> u32 {
    (x >> 8) & 0x1
}
/// `XHCI_HCC_SPC(x)`: Short packet.
pub const fn xhci_hcc_spc(x: u32) -> u32 {
    (x >> 9) & 0x1
}
/// `XHCI_HCC_SEC(x)`: Stopped EDTLA.
pub const fn xhci_hcc_sec(x: u32) -> u32 {
    (x >> 10) & 0x1
}
/// `XHCI_HCC_CFC(x)`: Contiguous Frame ID.
pub const fn xhci_hcc_cfc(x: u32) -> u32 {
    (x >> 11) & 0x1
}
/// `XHCI_HCC_MAX_PSA_SZ(x)`: Max pri. stream arr.
pub const fn xhci_hcc_max_psa_sz(x: u32) -> u32 {
    (x >> 12) & 0xf
}
/// `XHCI_HCC_XECP(x)`: Ext. capabilities.
pub const fn xhci_hcc_xecp(x: u32) -> u32 {
    (x >> 16) & 0xffff
}

/// `XHCI_DBOFF`: RO doorbell offset.
pub const XHCI_DBOFF: BusSize = 0x14;
/// `XHCI_RTSOFF`: RO runtime register space offset.
pub const XHCI_RTSOFF: BusSize = 0x18;

/*
 * XHCI operational registers.
 * Offset given by XHCI_CAPLENGTH register.
 */
/// `XHCI_USBCMD`: XHCI command.
pub const XHCI_USBCMD: BusSize = 0x00;
/// `XHCI_CMD_RS`: RW Run/Stop.
pub const XHCI_CMD_RS: u32 = 0x00000001;
/// `XHCI_CMD_HCRST`: RW Host Controller Reset.
pub const XHCI_CMD_HCRST: u32 = 0x00000002;
/// `XHCI_CMD_INTE`: RW Interrupter Enable.
pub const XHCI_CMD_INTE: u32 = 0x00000004;
/// `XHCI_CMD_HSEE`: RW Host System Error Enable.
pub const XHCI_CMD_HSEE: u32 = 0x00000008;
/// `XHCI_CMD_LHCRST`: RO/RW Light HC Reset.
pub const XHCI_CMD_LHCRST: u32 = 0x00000080;
/// `XHCI_CMD_CSS`: RW Controller Save State.
pub const XHCI_CMD_CSS: u32 = 0x00000100;
/// `XHCI_CMD_CRS`: RW Controller Restore State.
pub const XHCI_CMD_CRS: u32 = 0x00000200;
/// `XHCI_CMD_EWE`: RW Enable Wrap Event.
pub const XHCI_CMD_EWE: u32 = 0x00000400;
/// `XHCI_CMD_EU3S`: RW Enable U3 MFINDEX Stop.
pub const XHCI_CMD_EU3S: u32 = 0x00000800;

/// `XHCI_USBSTS`: XHCI status.
pub const XHCI_USBSTS: BusSize = 0x04;
/// `XHCI_STS_HCH`: RO - Host Controller Halted.
pub const XHCI_STS_HCH: u32 = 0x00000001;
/// `XHCI_STS_HSE`: RW - Host System Error.
pub const XHCI_STS_HSE: u32 = 0x00000004;
/// `XHCI_STS_EINT`: RW - Event Interrupt.
pub const XHCI_STS_EINT: u32 = 0x00000008;
/// `XHCI_STS_PCD`: RW - Port Change Detect.
pub const XHCI_STS_PCD: u32 = 0x00000010;
/// `XHCI_STS_SSS`: RO - Save State Status.
pub const XHCI_STS_SSS: u32 = 0x00000100;
/// `XHCI_STS_RSS`: RO - Restore State Status.
pub const XHCI_STS_RSS: u32 = 0x00000200;
/// `XHCI_STS_SRE`: RW - Save/Restore Error.
pub const XHCI_STS_SRE: u32 = 0x00000400;
/// `XHCI_STS_CNR`: RO - Controller Not Ready.
pub const XHCI_STS_CNR: u32 = 0x00000800;
/// `XHCI_STS_HCE`: RO - Host Controller Error.
pub const XHCI_STS_HCE: u32 = 0x00001000;

/// `XHCI_PAGESIZE`: XHCI page size mask.
pub const XHCI_PAGESIZE: BusSize = 0x08;
/// `XHCI_PAGESIZE_4K`: 4K Page Size.
pub const XHCI_PAGESIZE_4K: u32 = 0x00000001;
/// `XHCI_PAGESIZE_8K`: 8K Page Size.
pub const XHCI_PAGESIZE_8K: u32 = 0x00000002;
/// `XHCI_PAGESIZE_16K`: 16K Page Size.
pub const XHCI_PAGESIZE_16K: u32 = 0x00000004;
/// `XHCI_PAGESIZE_32K`: 32K Page Size.
pub const XHCI_PAGESIZE_32K: u32 = 0x00000008;
/// `XHCI_PAGESIZE_64K`: 64K Page Size.
pub const XHCI_PAGESIZE_64K: u32 = 0x00000010;

/// `XHCI_DNCTRL`: XHCI device notification control.
pub const XHCI_DNCTRL: BusSize = 0x14;
/// `XHCI_DNCTRL_MASK(n)`.
pub const fn xhci_dnctrl_mask(n: u32) -> u32 {
    1 << n
}

/// `XHCI_CRCR_LO`: XHCI command ring control.
pub const XHCI_CRCR_LO: BusSize = 0x18;
/// `XHCI_CRCR_LO_RCS`: RW - consumer cycle state.
pub const XHCI_CRCR_LO_RCS: u32 = 0x00000001;
/// `XHCI_CRCR_LO_CS`: RW - command stop.
pub const XHCI_CRCR_LO_CS: u32 = 0x00000002;
/// `XHCI_CRCR_LO_CA`: RW - command abort.
pub const XHCI_CRCR_LO_CA: u32 = 0x00000004;
/// `XHCI_CRCR_LO_CRR`: RW - command ring running.
pub const XHCI_CRCR_LO_CRR: u32 = 0x00000008;
/// `XHCI_CRCR_LO_MASK`.
pub const XHCI_CRCR_LO_MASK: u32 = 0x0000000F;

/// `XHCI_CRCR_HI`: XHCI command ring control.
pub const XHCI_CRCR_HI: BusSize = 0x1C;
/// `XHCI_DCBAAP_LO`: XHCI dev context BA pointer.
pub const XHCI_DCBAAP_LO: BusSize = 0x30;
/// `XHCI_DCBAAP_HI`: XHCI dev context BA pointer.
pub const XHCI_DCBAAP_HI: BusSize = 0x34;
/// `XHCI_CONFIG`.
pub const XHCI_CONFIG: BusSize = 0x38;
/// `XHCI_CONFIG_SLOTS_MASK`: RW - nb of device slots enabled.
pub const XHCI_CONFIG_SLOTS_MASK: u32 = 0x000000ff;

/*
 * XHCI port status registers.
 */
/// `XHCI_PORTSC(n)`: XHCI port status.
pub const fn xhci_portsc(n: usize) -> BusSize {
    0x3f0 + 0x10 * n
}
/// `XHCI_PS_CCS`: RO - current connect status.
pub const XHCI_PS_CCS: u32 = 0x00000001;
/// `XHCI_PS_PED`: RW - port enabled / disabled.
pub const XHCI_PS_PED: u32 = 0x00000002;
/// `XHCI_PS_OCA`: RO - over current active.
pub const XHCI_PS_OCA: u32 = 0x00000008;
/// `XHCI_PS_PR`: RW - port reset.
pub const XHCI_PS_PR: u32 = 0x00000010;
/// `XHCI_PS_GET_PLS(x)`: RW - port link state.
pub const fn xhci_ps_get_pls(x: u32) -> u32 {
    (x >> 5) & 0xf
}
/// `XHCI_PS_SET_PLS(x)`: RW - port link state.
pub const fn xhci_ps_set_pls(x: u32) -> u32 {
    (x & 0xf) << 5
}
/// `XHCI_PS_PP`: RW - port power.
pub const XHCI_PS_PP: u32 = 0x00000200;
/// `XHCI_PS_SPEED(x)`: RO - port speed.
pub const fn xhci_ps_speed(x: u32) -> u32 {
    (x >> 10) & 0xf
}
/// `XHCI_PS_GET_PIC(x)`: RW - port indicator.
pub const fn xhci_ps_get_pic(x: u32) -> u32 {
    (x >> 14) & 0x3
}
/// `XHCI_PS_SET_PIC(x)`: RW - port indicator.
pub const fn xhci_ps_set_pic(x: u32) -> u32 {
    (x & 0x3) << 14
}
/// `XHCI_PS_LWS`: RW - link state write strobe.
pub const XHCI_PS_LWS: u32 = 0x00010000;
/// `XHCI_PS_CSC`: RW - connect status change.
pub const XHCI_PS_CSC: u32 = 0x00020000;
/// `XHCI_PS_PEC`: RW - port enable/disable change.
pub const XHCI_PS_PEC: u32 = 0x00040000;
/// `XHCI_PS_WRC`: RW - warm port reset change.
pub const XHCI_PS_WRC: u32 = 0x00080000;
/// `XHCI_PS_OCC`: RW - over-current change.
pub const XHCI_PS_OCC: u32 = 0x00100000;
/// `XHCI_PS_PRC`: RW - port reset change.
pub const XHCI_PS_PRC: u32 = 0x00200000;
/// `XHCI_PS_PLC`: RW - port link state change.
pub const XHCI_PS_PLC: u32 = 0x00400000;
/// `XHCI_PS_CEC`: RW - config error change.
pub const XHCI_PS_CEC: u32 = 0x00800000;
/// `XHCI_PS_CAS`: RO - cold attach status.
pub const XHCI_PS_CAS: u32 = 0x01000000;
/// `XHCI_PS_WCE`: RW - wake on connect enable.
pub const XHCI_PS_WCE: u32 = 0x02000000;
/// `XHCI_PS_WDE`: RW - wake on disconnect enable.
pub const XHCI_PS_WDE: u32 = 0x04000000;
/// `XHCI_PS_WOE`: RW - wake on over-current enable.
pub const XHCI_PS_WOE: u32 = 0x08000000;
/// `XHCI_PS_DR`: RO - device removable.
pub const XHCI_PS_DR: u32 = 0x40000000;
/// `XHCI_PS_WPR`: RW - warm port reset.
pub const XHCI_PS_WPR: u32 = 0x80000000;
/// `XHCI_PS_CLEAR`: command bits.
pub const XHCI_PS_CLEAR: u32 = 0x80ff01ff;

/// `XHCI_PORTPMSC(n)`: XHCI status & ctrl.
pub const fn xhci_portpmsc(n: usize) -> BusSize {
    0x3f4 + 0x10 * n
}
/// `XHCI_PM3_U1TO(x)`: RW - U1 timeout.
pub const fn xhci_pm3_u1to(x: u32) -> u32 {
    x & 0xff
}
/// `XHCI_PM3_U2TO(x)`: RW - U2 timeout.
pub const fn xhci_pm3_u2to(x: u32) -> u32 {
    (x & 0xff) << 8
}
/// `XHCI_PM3_FLA`: RW - Force Link PM Accept.
pub const XHCI_PM3_FLA: u32 = 0x00010000;
/// `XHCI_PM2_L1S(x)`: RO - L1 status.
pub const fn xhci_pm2_l1s(x: u32) -> u32 {
    x & 0x7
}
/// `XHCI_PM2_RWE`: RW - remote wakeup enable.
pub const XHCI_PM2_RWE: u32 = 0x00000008;
/// `XHCI_PM2_HIRD(x)`: RW - resume duration.
pub const fn xhci_pm2_hird(x: u32) -> u32 {
    (x & 0xf) << 4
}
/// `XHCI_PM2_L1SLOT(x)`: RW - L1 device slot.
pub const fn xhci_pm2_l1slot(x: u32) -> u32 {
    (x & 0xff) << 8
}
/// `XHCI_PM2_HLE`: RW - hardware LPM enable.
pub const XHCI_PM2_HLE: u32 = 0x00010000;
/// `XHCI_PORTLI(n)`: XHCI port link info.
pub const fn xhci_portli(n: usize) -> BusSize {
    0x3f8 + 0x10 * n
}
/// `XHCI_PORTRSV(n)`: XHCI port reserved.
pub const fn xhci_portrsv(n: usize) -> BusSize {
    0x3fc + 0x10 * n
}

/*
 * XHCI runtime registers.
 * Offset given by XHCI_CAPLENGTH + XHCI_RTSOFF registers.
 */
/// `XHCI_MFINDEX`: RO - microframe index.
pub const XHCI_MFINDEX: BusSize = 0x0000;
/// `XHCI_GET_MFINDEX(x)`.
pub const fn xhci_get_mfindex(x: u32) -> u32 {
    x & 0x3fff
}
/// `XHCI_IMAN(n)`: intr.management.
pub const fn xhci_iman(n: usize) -> BusSize {
    0x0020 + 0x20 * n
}
/// `XHCI_IMAN_INTR_PEND`: RW - interrupt pending.
pub const XHCI_IMAN_INTR_PEND: u32 = 0x00000001;
/// `XHCI_IMAN_INTR_ENA`: RW - interrupt enable.
pub const XHCI_IMAN_INTR_ENA: u32 = 0x00000002;

/* XHCI interrupt moderation */
/// `XHCI_IMOD(n)`.
pub const fn xhci_imod(n: usize) -> BusSize {
    0x0024 + 0x20 * n
}
/// `XHCI_IMOD_IVAL_GET(x)`: 250ns unit.
pub const fn xhci_imod_ival_get(x: u32) -> u32 {
    x & 0xffff
}
/// `XHCI_IMOD_IVAL_SET(x)`: 250ns unit.
pub const fn xhci_imod_ival_set(x: u32) -> u32 {
    x & 0xffff
}
/// `XHCI_IMOD_ICNT_GET(x)`: 250ns unit.
pub const fn xhci_imod_icnt_get(x: u32) -> u32 {
    (x >> 16) & 0xffff
}
/// `XHCI_IMOD_ICNT_SET(x)`: 250ns unit.
pub const fn xhci_imod_icnt_set(x: u32) -> u32 {
    (x & 0xffff) << 16
}
/// `XHCI_IMOD_DEFAULT`: 8000 IRQ/second.
pub const XHCI_IMOD_DEFAULT: u32 = 0x000001F4;
/// `XHCI_IMOD_DEFAULT_LP`: 4000 IRQ/second.
pub const XHCI_IMOD_DEFAULT_LP: u32 = 0x000003E8;

/* XHCI event ring segment table size */
/// `XHCI_ERSTSZ(n)`.
pub const fn xhci_erstsz(n: usize) -> BusSize {
    0x0028 + 0x20 * n
}
/// `XHCI_ERSTS_SET(x)`.
pub const fn xhci_ersts_set(x: u32) -> u32 {
    x & 0xffff
}

/* XHCI event ring segment table BA */
/// `XHCI_ERSTBA_LO(n)`.
pub const fn xhci_erstba_lo(n: usize) -> BusSize {
    0x0030 + 0x20 * n
}
/// `XHCI_ERSTBA_HI(n)`.
pub const fn xhci_erstba_hi(n: usize) -> BusSize {
    0x0034 + 0x20 * n
}

/* XHCI event ring dequeue pointer */
/// `XHCI_ERDP_LO(n)`.
pub const fn xhci_erdp_lo(n: usize) -> BusSize {
    0x0038 + 0x20 * n
}
/// `XHCI_ERDP_LO_BUSY`: RW - event handler busy.
pub const XHCI_ERDP_LO_BUSY: u32 = 0x00000008;
/// `XHCI_ERDP_HI(n)`.
pub const fn xhci_erdp_hi(n: usize) -> BusSize {
    0x003c + 0x20 * n
}

/*
 * XHCI doorbell registers.
 * Offset given by XHCI_CAPLENGTH + XHCI_DBOFF registers.
 */
/// `XHCI_DOORBELL(n)`.
pub const fn xhci_doorbell(n: usize) -> BusSize {
    4 * n
}
/// `XHCI_DB_GET_SID(x)`: RW - stream ID.
pub const fn xhci_db_get_sid(x: u32) -> u32 {
    (x >> 16) & 0xffff
}
/// `XHCI_DB_SET_SID(x)`: RW - stream ID.
pub const fn xhci_db_set_sid(x: u32) -> u32 {
    (x & 0xffff) << 16
}

/* XHCI legacy support */
/// `XHCI_XECP_ID(x)`.
pub const fn xhci_xecp_id(x: u32) -> u32 {
    x & 0xff
}
/// `XHCI_XECP_NEXT(x)`.
pub const fn xhci_xecp_next(x: u32) -> u32 {
    (x >> 8) & 0xff
}
/// `XHCI_XECP_BIOS_SEM`.
pub const XHCI_XECP_BIOS_SEM: BusSize = 0x0002;
/// `XHCI_XECP_OS_SEM`.
pub const XHCI_XECP_OS_SEM: BusSize = 0x0003;

/* XHCI capability ID's */
/// `XHCI_ID_USB_LEGACY`.
pub const XHCI_ID_USB_LEGACY: u32 = 0x0001;
/// `XHCI_ID_PROTOCOLS`.
pub const XHCI_ID_PROTOCOLS: u32 = 0x0002;
/// `XHCI_ID_POWER_MGMT`.
pub const XHCI_ID_POWER_MGMT: u32 = 0x0003;
/// `XHCI_ID_VIRTUALIZATION`.
pub const XHCI_ID_VIRTUALIZATION: u32 = 0x0004;
/// `XHCI_ID_MSG_IRQ`.
pub const XHCI_ID_MSG_IRQ: u32 = 0x0005;
/// `XHCI_ID_USB_LOCAL_MEM`.
pub const XHCI_ID_USB_LOCAL_MEM: u32 = 0x0006;

/// `struct xhci_erseg`: an event ring segment table entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XhciErseg {
    /// `er_addr`.
    pub er_addr: u64,
    /// `er_size`.
    pub er_size: u32,
    /// `er_rsvd`.
    pub er_rsvd: u32,
}

/// `struct xhci_sctx`: a slot context.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XhciSctx {
    /// `info_lo`: `XHCI_SCTX_ROUTE`, `_SPEED`, `_MTT`, `_HUB`, `_DCI`.
    pub info_lo: u32,
    /// `info_hi`: `XHCI_SCTX_MAX_EL`, `_RHPORT`, `_NPORTS`.
    pub info_hi: u32,
    /// `tt`: `XHCI_SCTX_TT_*`, `XHCI_SCTX_*_IRQ_TARGET`.
    pub tt: u32,
    /// `state`: `XHCI_SCTX_DEV_ADDR`, `_SLOT_STATE`.
    pub state: u32,
    /// `rsvd`.
    pub rsvd: [u32; 4],
}

/// `XHCI_SCTX_ROUTE(x)`.
pub const fn xhci_sctx_route(x: u32) -> u32 {
    x & 0xfffff
}
/// `XHCI_SCTX_SPEED(x)`.
pub const fn xhci_sctx_speed(x: u32) -> u32 {
    (x & 0xf) << 20
}
/// `XHCI_SCTX_MTT(x)`.
pub const fn xhci_sctx_mtt(x: u32) -> u32 {
    (x & 0x1) << 25
}
/// `XHCI_SCTX_HUB(x)`.
pub const fn xhci_sctx_hub(x: u32) -> u32 {
    (x & 0x1) << 26
}
/// `XHCI_SCTX_DCI(x)`.
pub const fn xhci_sctx_dci(x: u32) -> u32 {
    (x & 0x1f) << 27
}
/// `XHCI_SCTX_MAX_EL(x)`.
pub const fn xhci_sctx_max_el(x: u32) -> u32 {
    x & 0xffff
}
/// `XHCI_SCTX_RHPORT(x)`.
pub const fn xhci_sctx_rhport(x: u32) -> u32 {
    (x & 0xff) << 16
}
/// `XHCI_SCTX_NPORTS(x)`.
pub const fn xhci_sctx_nports(x: u32) -> u32 {
    (x & 0xff) << 24
}
/// `XHCI_SCTX_TT_HUB_SID(x)`.
pub const fn xhci_sctx_tt_hub_sid(x: u32) -> u32 {
    x & 0xff
}
/// `XHCI_SCTX_TT_PORT_NUM(x)`.
pub const fn xhci_sctx_tt_port_num(x: u32) -> u32 {
    (x & 0xff) << 8
}
/// `XHCI_SCTX_TT_THINK_TIME(x)`.
pub const fn xhci_sctx_tt_think_time(x: u32) -> u32 {
    (x & 0x3) << 16
}
/// `XHCI_SCTX_SET_IRQ_TARGET(x)`.
pub const fn xhci_sctx_set_irq_target(x: u32) -> u32 {
    (x & 0x3ff) << 22
}
/// `XHCI_SCTX_GET_IRQ_TARGET(x)`.
pub const fn xhci_sctx_get_irq_target(x: u32) -> u32 {
    (x >> 22) & 0x3ff
}
/// `XHCI_SCTX_DEV_ADDR(x)`.
pub const fn xhci_sctx_dev_addr(x: u32) -> u32 {
    x & 0xff
}
/// `XHCI_SCTX_SLOT_STATE(x)`.
pub const fn xhci_sctx_slot_state(x: u32) -> u32 {
    (x >> 27) & 0x1f
}

/// `struct xhci_epctx`: an endpoint context.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XhciEpctx {
    /// `info_lo`: `XHCI_EPCTX_STATE`, `_MULT`, `_MAXP_STREAMS`, `_LSA`, `_IVAL`.
    pub info_lo: u32,
    /// `info_hi`: `XHCI_EPCTX_*_CERR`, `_EPTYPE`, `_HID`, `_MAXB`, `_MPS`.
    pub info_hi: u32,
    /// `deqp`: the TR dequeue pointer and its cycle bit.
    pub deqp: u64,
    /// `txinfo`: `XHCI_EPCTX_AVG_TRB_LEN`, `_MAX_ESIT_PAYLOAD`.
    pub txinfo: u32,
    /// `rsvd`.
    pub rsvd: [u32; 3],
}

/// `XHCI_EPCTX_STATE(x)`.
pub const fn xhci_epctx_state(x: u32) -> u32 {
    x & 0x7
}
/// `XHCI_EP_DISABLED`.
pub const XHCI_EP_DISABLED: u32 = 0x0;
/// `XHCI_EP_RUNNING`.
pub const XHCI_EP_RUNNING: u32 = 0x1;
/// `XHCI_EP_HALTED`.
pub const XHCI_EP_HALTED: u32 = 0x2;
/// `XHCI_EP_STOPPED`.
pub const XHCI_EP_STOPPED: u32 = 0x3;
/// `XHCI_EP_ERROR`.
pub const XHCI_EP_ERROR: u32 = 0x4;
/// `XHCI_EPCTX_SET_MULT(x)`.
pub const fn xhci_epctx_set_mult(x: u32) -> u32 {
    (x & 0x3) << 8
}
/// `XHCI_EPCTX_GET_MULT(x)`.
pub const fn xhci_epctx_get_mult(x: u32) -> u32 {
    (x >> 8) & 0x3
}
/// `XHCI_EPCTX_SET_MAXP_STREAMS(x)`.
pub const fn xhci_epctx_set_maxp_streams(x: u32) -> u32 {
    (x & 0x1F) << 10
}
/// `XHCI_EPCTX_GET_MAXP_STREAMS(x)`.
pub const fn xhci_epctx_get_maxp_streams(x: u32) -> u32 {
    (x >> 10) & 0x1F
}
/// `XHCI_EPCTX_SET_LSA(x)`.
pub const fn xhci_epctx_set_lsa(x: u32) -> u32 {
    (x & 0x1) << 15
}
/// `XHCI_EPCTX_GET_LSA(x)`.
pub const fn xhci_epctx_get_lsa(x: u32) -> u32 {
    (x >> 15) & 0x1
}
/// `XHCI_EPCTX_SET_IVAL(x)`.
pub const fn xhci_epctx_set_ival(x: u32) -> u32 {
    (x & 0xff) << 16
}
/// `XHCI_EPCTX_GET_IVAL(x)`.
pub const fn xhci_epctx_get_ival(x: u32) -> u32 {
    (x >> 16) & 0xFF
}
/// `XHCI_EPCTX_MAX_IVAL`: Poll rates: 2^(n-1) * 0.125us.
pub const XHCI_EPCTX_MAX_IVAL: u32 = 15;
/// `XHCI_EPCTX_SET_CERR(x)`.
pub const fn xhci_epctx_set_cerr(x: u32) -> u32 {
    (x & 0x3) << 1
}
/// `XHCI_EPCTX_SET_EPTYPE(x)`.
pub const fn xhci_epctx_set_eptype(x: u32) -> u32 {
    (x & 0x7) << 3
}
/// `XHCI_EPCTX_GET_EPTYPE(x)`.
pub const fn xhci_epctx_get_eptype(x: u32) -> u32 {
    (x >> 3) & 0x7
}
/// `XHCI_EPCTX_SET_HID(x)`.
pub const fn xhci_epctx_set_hid(x: u32) -> u32 {
    (x & 0x1) << 7
}
/// `XHCI_EPCTX_GET_HID(x)`.
pub const fn xhci_epctx_get_hid(x: u32) -> u32 {
    (x >> 7) & 0x1
}
/// `XHCI_EPCTX_SET_MAXB(x)`.
pub const fn xhci_epctx_set_maxb(x: u32) -> u32 {
    (x & 0xff) << 8
}
/// `XHCI_EPCTX_GET_MAXB(x)`.
pub const fn xhci_epctx_get_maxb(x: u32) -> u32 {
    (x >> 8) & 0xff
}
/// `XHCI_EPCTX_SET_MPS(x)`.
pub const fn xhci_epctx_set_mps(x: u32) -> u32 {
    (x & 0xffff) << 16
}
/// `XHCI_EPCTX_GET_MPS(x)`.
pub const fn xhci_epctx_get_mps(x: u32) -> u32 {
    (x >> 16) & 0xffff
}
/// `XHCI_SPEED_FULL`.
pub const XHCI_SPEED_FULL: u32 = 1;
/// `XHCI_SPEED_LOW`.
pub const XHCI_SPEED_LOW: u32 = 2;
/// `XHCI_SPEED_HIGH`.
pub const XHCI_SPEED_HIGH: u32 = 3;
/// `XHCI_SPEED_SUPER`.
pub const XHCI_SPEED_SUPER: u32 = 4;
/// `XHCI_EPCTX_AVG_TRB_LEN(x)`.
pub const fn xhci_epctx_avg_trb_len(x: u32) -> u32 {
    x & 0xffff
}
/// `XHCI_EPCTX_MAX_ESIT_PAYLOAD(x)`.
pub const fn xhci_epctx_max_esit_payload(x: u32) -> u32 {
    (x & 0xffff) << 16
}

/// `struct xhci_inctx`: an input control context.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XhciInctx {
    /// `drop_flags`.
    pub drop_flags: u32,
    /// `add_flags`.
    pub add_flags: u32,
    /// `rsvd`.
    pub rsvd: [u32; 6],
}

/// `XHCI_INCTX_MASK_DCI(n)`.
pub const fn xhci_inctx_mask_dci(n: u32) -> u32 {
    0x1 << n
}

/// `struct xhci_trb`: a transfer request block, on every ring.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XhciTrb {
    /// `trb_paddr`: a buffer address, a SETUP packet (`XHCI_TRB_IDT`) or a TRB address.
    pub trb_paddr: u64,
    /// `trb_status`.
    pub trb_status: u32,
    /// `trb_flags`.
    pub trb_flags: u32,
}

/// `XHCI_TRB_PORTID(x)`: Port ID.
pub const fn xhci_trb_portid(x: u64) -> u32 {
    ((x >> 24) & 0xff) as u32
}
/// `XHCI_TRB_MAXSIZE`.
pub const XHCI_TRB_MAXSIZE: u32 = 64 * 1024;

/// `XHCI_TRB_GET_CODE(x)`.
pub const fn xhci_trb_get_code(x: u32) -> u32 {
    (x >> 24) & 0xff
}
/// `XHCI_TRB_TDREM(x)`: TD remaining len.
pub const fn xhci_trb_tdrem(x: u32) -> u32 {
    (x & 0x1f) << 17
}
/// `XHCI_TRB_REMAIN(x)`: Remaining length.
pub const fn xhci_trb_remain(x: u32) -> u32 {
    x & 0xffffff
}
/// `XHCI_TRB_LEN(x)`: Transfer length.
pub const fn xhci_trb_len(x: u32) -> u32 {
    x & 0x1ffff
}
/// `XHCI_TRB_INTR(x)`: MSI-X intr. target.
pub const fn xhci_trb_intr(x: u32) -> u32 {
    (x & 0x3ff) << 22
}

/// `XHCI_TRB_CYCLE`: Enqueue point of xfer ring.
pub const XHCI_TRB_CYCLE: u32 = 1 << 0;
/// `XHCI_TRB_ENT`: Evaluate next TRB.
pub const XHCI_TRB_ENT: u32 = 1 << 1;
/// `XHCI_TRB_LINKSEG`: Link to next segment.
pub const XHCI_TRB_LINKSEG: u32 = XHCI_TRB_ENT;
/// `XHCI_TRB_ISP`: Interrupt on short packet.
pub const XHCI_TRB_ISP: u32 = 1 << 2;
/// `XHCI_TRB_NOSNOOP`: PCIe no snoop.
pub const XHCI_TRB_NOSNOOP: u32 = 1 << 3;
/// `XHCI_TRB_CHAIN`: Chained with next TRB.
pub const XHCI_TRB_CHAIN: u32 = 1 << 4;
/// `XHCI_TRB_IOC`: Interrupt On Completion.
pub const XHCI_TRB_IOC: u32 = 1 << 5;
/// `XHCI_TRB_IDT`: Immediate DaTa.
pub const XHCI_TRB_IDT: u32 = 1 << 6;
/// `XHCI_TRB_ISOC_TBC(x)`: Transfer Burst Count.
pub const fn xhci_trb_isoc_tbc(x: u32) -> u32 {
    (x & 0x3) << 7
}
/// `XHCI_TRB_BSR`: Block Set Address Request.
pub const XHCI_TRB_BSR: u32 = 1 << 9;
/// `XHCI_TRB_ISOC_BEI`: Block Event Interrupt.
pub const XHCI_TRB_ISOC_BEI: u32 = 1 << 9;
/// `XHCI_TRB_DIR_IN`.
pub const XHCI_TRB_DIR_IN: u32 = 1 << 16;
/// `XHCI_TRB_TRT_OUT`.
pub const XHCI_TRB_TRT_OUT: u32 = 2 << 16;
/// `XHCI_TRB_TRT_IN`.
pub const XHCI_TRB_TRT_IN: u32 = 3 << 16;
/// `XHCI_TRB_GET_EP(x)`.
pub const fn xhci_trb_get_ep(x: u32) -> u32 {
    (x >> 16) & 0x1f
}
/// `XHCI_TRB_SET_EP(x)`.
pub const fn xhci_trb_set_ep(x: u32) -> u32 {
    (x & 0x1f) << 16
}
/// `XHCI_TRB_ISOC_TLBPC(x)`.
pub const fn xhci_trb_isoc_tlbpc(x: u32) -> u32 {
    (x & 0xf) << 16
}
/// `XHCI_TRB_ISOC_FRAME(x)`.
pub const fn xhci_trb_isoc_frame(x: u32) -> u32 {
    (x & 0x7ff) << 20
}
/// `XHCI_TRB_GET_SLOT(x)`.
pub const fn xhci_trb_get_slot(x: u32) -> u32 {
    (x >> 24) & 0xff
}
/// `XHCI_TRB_SET_SLOT(x)`.
pub const fn xhci_trb_set_slot(x: u32) -> u32 {
    (x & 0xff) << 24
}
/// `XHCI_TRB_SIA`.
pub const XHCI_TRB_SIA: u32 = 1 << 31;

/// `XHCI_TRB_FLAGS_BITMASK`: the `%b` description of `trb_flags`.
pub const XHCI_TRB_FLAGS_BITMASK: &[u8] = b"\x10\x20SIA\x12TRT_OUT\x11DIR_IN\x0aBSR\x07IDT\
\x06IOC\x05CHAIN\x04NOSNOOP\x03ISP\x02LINKSEG\x01CYCLE";

/// `XHCI_TRB_TYPE_MASK`.
pub const XHCI_TRB_TYPE_MASK: u32 = 0xfc00;
/// `XHCI_TRB_TYPE(x)`.
pub const fn xhci_trb_type(x: u32) -> u32 {
    (x & XHCI_TRB_TYPE_MASK) >> 10
}

/* Transfer Ring Types */
/// `XHCI_TRB_TYPE_NORMAL`.
pub const XHCI_TRB_TYPE_NORMAL: u32 = 1 << 10;
/// `XHCI_TRB_TYPE_SETUP`: Setup stage (ctrl only).
pub const XHCI_TRB_TYPE_SETUP: u32 = 2 << 10;
/// `XHCI_TRB_TYPE_DATA`: Data stage (ctrl only).
pub const XHCI_TRB_TYPE_DATA: u32 = 3 << 10;
/// `XHCI_TRB_TYPE_STATUS`: Status stage (ctrl only).
pub const XHCI_TRB_TYPE_STATUS: u32 = 4 << 10;
/// `XHCI_TRB_TYPE_ISOCH`.
pub const XHCI_TRB_TYPE_ISOCH: u32 = 5 << 10;
/// `XHCI_TRB_TYPE_LINK`: Link next seg. (all+cmd).
pub const XHCI_TRB_TYPE_LINK: u32 = 6 << 10;
/// `XHCI_TRB_TYPE_EVENT`: Generate event (all).
pub const XHCI_TRB_TYPE_EVENT: u32 = 7 << 10;
/// `XHCI_TRB_TYPE_NOOP`: No-Op (all).
pub const XHCI_TRB_TYPE_NOOP: u32 = 8 << 10;

/* Command ring Types */
/// `XHCI_CMD_ENABLE_SLOT`.
pub const XHCI_CMD_ENABLE_SLOT: u32 = 9 << 10;
/// `XHCI_CMD_DISABLE_SLOT`.
pub const XHCI_CMD_DISABLE_SLOT: u32 = 10 << 10;
/// `XHCI_CMD_ADDRESS_DEVICE`.
pub const XHCI_CMD_ADDRESS_DEVICE: u32 = 11 << 10;
/// `XHCI_CMD_CONFIG_EP`.
pub const XHCI_CMD_CONFIG_EP: u32 = 12 << 10;
/// `XHCI_CMD_EVAL_CTX`.
pub const XHCI_CMD_EVAL_CTX: u32 = 13 << 10;
/// `XHCI_CMD_RESET_EP`.
pub const XHCI_CMD_RESET_EP: u32 = 14 << 10;
/// `XHCI_CMD_STOP_EP`.
pub const XHCI_CMD_STOP_EP: u32 = 15 << 10;
/// `XHCI_CMD_SET_TR_DEQ`.
pub const XHCI_CMD_SET_TR_DEQ: u32 = 16 << 10;
/// `XHCI_CMD_RESET_DEV`.
pub const XHCI_CMD_RESET_DEV: u32 = 17 << 10;
/// `XHCI_CMD_FEVENT`.
pub const XHCI_CMD_FEVENT: u32 = 18 << 10;
/// `XHCI_CMD_NEG_BW`: Negotiate bandwidth.
pub const XHCI_CMD_NEG_BW: u32 = 19 << 10;
/// `XHCI_CMD_SET_LT`: Set latency tolerance.
pub const XHCI_CMD_SET_LT: u32 = 20 << 10;
/// `XHCI_CMD_GET_BW`: Get port bandwidth.
pub const XHCI_CMD_GET_BW: u32 = 21 << 10;
/// `XHCI_CMD_FHEADER`.
pub const XHCI_CMD_FHEADER: u32 = 22 << 10;
/// `XHCI_CMD_NOOP`: To test the command ring.
pub const XHCI_CMD_NOOP: u32 = 23 << 10;

/* Event ring Types */
/// `XHCI_EVT_XFER`: Transfer event.
pub const XHCI_EVT_XFER: u32 = 32 << 10;
/// `XHCI_EVT_CMD_COMPLETE`.
pub const XHCI_EVT_CMD_COMPLETE: u32 = 33 << 10;
/// `XHCI_EVT_PORT_CHANGE`: Port status change.
pub const XHCI_EVT_PORT_CHANGE: u32 = 34 << 10;
/// `XHCI_EVT_BW_REQUEST`.
pub const XHCI_EVT_BW_REQUEST: u32 = 35 << 10;
/// `XHCI_EVT_DOORBELL`.
pub const XHCI_EVT_DOORBELL: u32 = 36 << 10;
/// `XHCI_EVT_HOST_CTRL`.
pub const XHCI_EVT_HOST_CTRL: u32 = 37 << 10;
/// `XHCI_EVT_DEVICE_NOTIFY`.
pub const XHCI_EVT_DEVICE_NOTIFY: u32 = 38 << 10;
/// `XHCI_EVT_MFINDEX_WRAP`.
pub const XHCI_EVT_MFINDEX_WRAP: u32 = 39 << 10;

/* TRB Completion codes */
/// `XHCI_CODE_INVALID`: Producer didn't update the code.
pub const XHCI_CODE_INVALID: u32 = 0;
/// `XHCI_CODE_SUCCESS`: Badaboum, plaf, plouf, yeepee!
pub const XHCI_CODE_SUCCESS: u32 = 1;
/// `XHCI_CODE_DATA_BUF`: Overrun or underrun.
pub const XHCI_CODE_DATA_BUF: u32 = 2;
/// `XHCI_CODE_BABBLE`: Device is "babbling".
pub const XHCI_CODE_BABBLE: u32 = 3;
/// `XHCI_CODE_TXERR`: USB Transaction error.
pub const XHCI_CODE_TXERR: u32 = 4;
/// `XHCI_CODE_TRB`: Invalid TRB.
pub const XHCI_CODE_TRB: u32 = 5;
/// `XHCI_CODE_STALL`: Stall condition.
pub const XHCI_CODE_STALL: u32 = 6;
/// `XHCI_CODE_RESOURCE`: No resource available for the cmd.
pub const XHCI_CODE_RESOURCE: u32 = 7;
/// `XHCI_CODE_BANDWIDTH`: Not enough bandwidth for the cmd.
pub const XHCI_CODE_BANDWIDTH: u32 = 8;
/// `XHCI_CODE_NO_SLOTS`: MaxSlots limit reached.
pub const XHCI_CODE_NO_SLOTS: u32 = 9;
/// `XHCI_CODE_STREAM_TYPE`: Stream Context Type value detected.
pub const XHCI_CODE_STREAM_TYPE: u32 = 10;
/// `XHCI_CODE_SLOT_NOT_ON`: Related device slot is disabled.
pub const XHCI_CODE_SLOT_NOT_ON: u32 = 11;
/// `XHCI_CODE_ENDP_NOT_ON`: Related endpoint is disabled.
pub const XHCI_CODE_ENDP_NOT_ON: u32 = 12;
/// `XHCI_CODE_SHORT_XFER`: Short packet.
pub const XHCI_CODE_SHORT_XFER: u32 = 13;
/// `XHCI_CODE_RING_UNDERRUN`: Empty ring when transmitting isoc.
pub const XHCI_CODE_RING_UNDERRUN: u32 = 14;
/// `XHCI_CODE_RING_OVERRUN`: Empty ring when receiving isoc.
pub const XHCI_CODE_RING_OVERRUN: u32 = 15;
/// `XHCI_CODE_VF_RING_FULL`: VF's event ring is full.
pub const XHCI_CODE_VF_RING_FULL: u32 = 16;
/// `XHCI_CODE_PARAMETER`: Context parameter is invalid.
pub const XHCI_CODE_PARAMETER: u32 = 17;
/// `XHCI_CODE_BW_OVERRUN`: TD exceeds the bandwidth.
pub const XHCI_CODE_BW_OVERRUN: u32 = 18;
/// `XHCI_CODE_CONTEXT_STATE`: Transition from illegal ctx state.
pub const XHCI_CODE_CONTEXT_STATE: u32 = 19;
/// `XHCI_CODE_NO_PING_RESP`: Unable to complete periodic xfer.
pub const XHCI_CODE_NO_PING_RESP: u32 = 20;
/// `XHCI_CODE_EV_RING_FULL`: Unable to post an evt to the ring.
pub const XHCI_CODE_EV_RING_FULL: u32 = 21;
/// `XHCI_CODE_INCOMPAT_DEV`: Device cannot be accessed.
pub const XHCI_CODE_INCOMPAT_DEV: u32 = 22;
/// `XHCI_CODE_MISSED_SRV`: Unable to service isoc EP in ESIT.
pub const XHCI_CODE_MISSED_SRV: u32 = 23;
/// `XHCI_CODE_CMD_RING_STOP`: Command Stop (CS) requested.
pub const XHCI_CODE_CMD_RING_STOP: u32 = 24;
/// `XHCI_CODE_CMD_ABORTED`: Command Abort (CA) operation.
pub const XHCI_CODE_CMD_ABORTED: u32 = 25;
/// `XHCI_CODE_XFER_STOPPED`: xfer terminated by a stop endpoint.
pub const XHCI_CODE_XFER_STOPPED: u32 = 26;
/// `XHCI_CODE_XFER_STOPINV`: TRB transfer length invalid.
pub const XHCI_CODE_XFER_STOPINV: u32 = 27;
/// `XHCI_CODE_XFER_SHORTPKT`: Stopped before reaching end of TD.
pub const XHCI_CODE_XFER_SHORTPKT: u32 = 28;
/// `XHCI_CODE_MELAT`: Max Exit Latency too large.
pub const XHCI_CODE_MELAT: u32 = 29;
/// `XHCI_CODE_RESERVED`.
pub const XHCI_CODE_RESERVED: u32 = 30;
/// `XHCI_CODE_ISOC_OVERRUN`: IN data buffer < Max ESIT Payload.
pub const XHCI_CODE_ISOC_OVERRUN: u32 = 31;
/// `XHCI_CODE_EVENT_LOST`: Internal overrun - impl. specific.
pub const XHCI_CODE_EVENT_LOST: u32 = 32;
/// `XHCI_CODE_UNDEFINED`: Fatal error - impl. specific.
pub const XHCI_CODE_UNDEFINED: u32 = 33;
/// `XHCI_CODE_INVALID_SID`: Invalid stream ID received.
pub const XHCI_CODE_INVALID_SID: u32 = 34;
/// `XHCI_CODE_SEC_BW`: Cannot alloc secondary BW Domain.
pub const XHCI_CODE_SEC_BW: u32 = 35;
/// `XHCI_CODE_SPLITERR`: USB2 split transaction.
pub const XHCI_CODE_SPLITERR: u32 = 36;

const _: () = {
    assert!(size_of::<XhciErseg>() == 16);
    assert!(size_of::<XhciSctx>() == 32);
    assert!(size_of::<XhciEpctx>() == 32);
    assert!(size_of::<XhciInctx>() == 32);
    assert!(size_of::<XhciTrb>() == 16);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_macros() {
        assert_eq!(xhci_portsc(1), 0x400);
        assert_eq!(xhci_portsc(4), 0x430);
        assert_eq!(xhci_iman(0), 0x20);
        assert_eq!(xhci_imod(0), 0x24);
        assert_eq!(xhci_erstsz(0), 0x28);
        assert_eq!(xhci_erstba_lo(0), 0x30);
        assert_eq!(xhci_erstba_hi(0), 0x34);
        assert_eq!(xhci_erdp_lo(0), 0x38);
        assert_eq!(xhci_erdp_hi(0), 0x3c);
        assert_eq!(xhci_doorbell(3), 12);
    }

    #[test]
    fn parameter_fields() {
        // QEMU's qemu-xhci: 64 slots, 16 interrupters, 8 ports.
        let hcs1 = 0x0800_1040;
        assert_eq!(xhci_hcs1_devslot_max(hcs1), 0x40);
        assert_eq!(xhci_hcs1_irq_max(hcs1), 0x10);
        assert_eq!(xhci_hcs1_n_ports(hcs1), 8);
        // Scratchpad count: hi 5 bits at 21..25, lo 5 bits at 27..31.
        assert_eq!(xhci_hcs2_spb_max(0x0020_0000), 0x20);
        assert_eq!(xhci_hcs2_spb_max(0x0800_0000), 1);
        assert!(xhci_hcs2_ist_micro(0x7));
        assert!(!xhci_hcs2_ist_micro(0x8));
    }

    #[test]
    fn trb_fields() {
        let flags = xhci_trb_set_slot(5) | xhci_trb_set_ep(3) | XHCI_CMD_STOP_EP;
        assert_eq!(xhci_trb_get_slot(flags), 5);
        assert_eq!(xhci_trb_get_ep(flags), 3);
        assert_eq!(flags & XHCI_TRB_TYPE_MASK, XHCI_CMD_STOP_EP);
        assert_eq!(xhci_trb_type(XHCI_EVT_PORT_CHANGE), 34);
        assert_eq!(xhci_trb_portid(0x0300_0000), 3);
        assert_eq!(xhci_trb_get_code(0x0d00_0004), XHCI_CODE_SHORT_XFER);
        assert_eq!(xhci_trb_remain(0x0d00_0004), 4);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/usb/xhcireg.h");
        let mut ours = crate::reftest::assert_defines!(defs;
            XHCI_DCBAA_ALIGN, XHCI_ICTX_ALIGN, XHCI_SCTX_ALIGN, XHCI_OCTX_ALIGN,
            XHCI_XFER_RING_ALIGN, XHCI_CMDS_RING_ALIGN, XHCI_EVTS_RING_ALIGN, XHCI_RING_BOUNDARY,
            XHCI_ERST_ALIGN, XHCI_ERST_BOUNDARY, XHCI_SPAD_TABLE_ALIGN,
            PCI_CBMEM, PCI_INTERFACE_XHCI, PCI_USBREV, PCI_USBREV_MASK, PCI_USBREV_3_0,
            PCI_XHCI_FLADJ, PCI_XHCI_INTEL_XUSB2PR, PCI_XHCI_INTEL_XUSB2PRM,
            PCI_XHCI_INTEL_USB3_PSSEN, PCI_XHCI_INTEL_USB3PRM,
            XHCI_CAPLENGTH, XHCI_RESERVED, XHCI_HCIVERSION, XHCI_HCIVERSION_0_9,
            XHCI_HCIVERSION_1_0, XHCI_HCSPARAMS1, XHCI_HCSPARAMS2, XHCI_HCSPARAMS3, XHCI_HCCPARAMS,
            XHCI_DBOFF, XHCI_RTSOFF,
            XHCI_USBCMD, XHCI_CMD_RS, XHCI_CMD_HCRST, XHCI_CMD_INTE, XHCI_CMD_HSEE,
            XHCI_CMD_LHCRST, XHCI_CMD_CSS, XHCI_CMD_CRS, XHCI_CMD_EWE, XHCI_CMD_EU3S,
            XHCI_USBSTS, XHCI_STS_HCH, XHCI_STS_HSE, XHCI_STS_EINT, XHCI_STS_PCD, XHCI_STS_SSS,
            XHCI_STS_RSS, XHCI_STS_SRE, XHCI_STS_CNR, XHCI_STS_HCE,
            XHCI_PAGESIZE, XHCI_PAGESIZE_4K, XHCI_PAGESIZE_8K, XHCI_PAGESIZE_16K,
            XHCI_PAGESIZE_32K, XHCI_PAGESIZE_64K, XHCI_DNCTRL,
            XHCI_CRCR_LO, XHCI_CRCR_LO_RCS, XHCI_CRCR_LO_CS, XHCI_CRCR_LO_CA, XHCI_CRCR_LO_CRR,
            XHCI_CRCR_LO_MASK, XHCI_CRCR_HI, XHCI_DCBAAP_LO, XHCI_DCBAAP_HI, XHCI_CONFIG,
            XHCI_CONFIG_SLOTS_MASK,
            XHCI_PS_CCS, XHCI_PS_PED, XHCI_PS_OCA, XHCI_PS_PR, XHCI_PS_PP, XHCI_PS_LWS,
            XHCI_PS_CSC, XHCI_PS_PEC, XHCI_PS_WRC, XHCI_PS_OCC, XHCI_PS_PRC, XHCI_PS_PLC,
            XHCI_PS_CEC, XHCI_PS_CAS, XHCI_PS_WCE, XHCI_PS_WDE, XHCI_PS_WOE, XHCI_PS_DR,
            XHCI_PS_WPR, XHCI_PS_CLEAR, XHCI_PM3_FLA, XHCI_PM2_RWE, XHCI_PM2_HLE,
            XHCI_MFINDEX, XHCI_IMAN_INTR_PEND, XHCI_IMAN_INTR_ENA, XHCI_IMOD_DEFAULT,
            XHCI_IMOD_DEFAULT_LP, XHCI_ERDP_LO_BUSY,
            XHCI_XECP_BIOS_SEM, XHCI_XECP_OS_SEM,
            XHCI_ID_USB_LEGACY, XHCI_ID_PROTOCOLS, XHCI_ID_POWER_MGMT, XHCI_ID_VIRTUALIZATION,
            XHCI_ID_MSG_IRQ, XHCI_ID_USB_LOCAL_MEM,
            XHCI_EP_DISABLED, XHCI_EP_RUNNING, XHCI_EP_HALTED, XHCI_EP_STOPPED, XHCI_EP_ERROR,
            XHCI_EPCTX_MAX_IVAL, XHCI_SPEED_FULL, XHCI_SPEED_LOW, XHCI_SPEED_HIGH,
            XHCI_SPEED_SUPER,
            XHCI_TRB_MAXSIZE, XHCI_TRB_CYCLE, XHCI_TRB_ENT, XHCI_TRB_LINKSEG, XHCI_TRB_ISP,
            XHCI_TRB_NOSNOOP, XHCI_TRB_CHAIN, XHCI_TRB_IOC, XHCI_TRB_IDT, XHCI_TRB_BSR,
            XHCI_TRB_ISOC_BEI, XHCI_TRB_DIR_IN, XHCI_TRB_TRT_OUT, XHCI_TRB_TRT_IN, XHCI_TRB_SIA,
            XHCI_TRB_TYPE_MASK,
            XHCI_TRB_TYPE_NORMAL, XHCI_TRB_TYPE_SETUP, XHCI_TRB_TYPE_DATA, XHCI_TRB_TYPE_STATUS,
            XHCI_TRB_TYPE_ISOCH, XHCI_TRB_TYPE_LINK, XHCI_TRB_TYPE_EVENT, XHCI_TRB_TYPE_NOOP,
            XHCI_CMD_ENABLE_SLOT, XHCI_CMD_DISABLE_SLOT, XHCI_CMD_ADDRESS_DEVICE,
            XHCI_CMD_CONFIG_EP, XHCI_CMD_EVAL_CTX, XHCI_CMD_RESET_EP, XHCI_CMD_STOP_EP,
            XHCI_CMD_SET_TR_DEQ, XHCI_CMD_RESET_DEV, XHCI_CMD_FEVENT, XHCI_CMD_NEG_BW,
            XHCI_CMD_SET_LT, XHCI_CMD_GET_BW, XHCI_CMD_FHEADER, XHCI_CMD_NOOP,
            XHCI_EVT_XFER, XHCI_EVT_CMD_COMPLETE, XHCI_EVT_PORT_CHANGE, XHCI_EVT_BW_REQUEST,
            XHCI_EVT_DOORBELL, XHCI_EVT_HOST_CTRL, XHCI_EVT_DEVICE_NOTIFY, XHCI_EVT_MFINDEX_WRAP,
            XHCI_CODE_INVALID, XHCI_CODE_SUCCESS, XHCI_CODE_DATA_BUF, XHCI_CODE_BABBLE,
            XHCI_CODE_TXERR, XHCI_CODE_TRB, XHCI_CODE_STALL, XHCI_CODE_RESOURCE,
            XHCI_CODE_BANDWIDTH, XHCI_CODE_NO_SLOTS, XHCI_CODE_STREAM_TYPE, XHCI_CODE_SLOT_NOT_ON,
            XHCI_CODE_ENDP_NOT_ON, XHCI_CODE_SHORT_XFER, XHCI_CODE_RING_UNDERRUN,
            XHCI_CODE_RING_OVERRUN, XHCI_CODE_VF_RING_FULL, XHCI_CODE_PARAMETER,
            XHCI_CODE_BW_OVERRUN, XHCI_CODE_CONTEXT_STATE, XHCI_CODE_NO_PING_RESP,
            XHCI_CODE_EV_RING_FULL, XHCI_CODE_INCOMPAT_DEV, XHCI_CODE_MISSED_SRV,
            XHCI_CODE_CMD_RING_STOP, XHCI_CODE_CMD_ABORTED, XHCI_CODE_XFER_STOPPED,
            XHCI_CODE_XFER_STOPINV, XHCI_CODE_XFER_SHORTPKT, XHCI_CODE_MELAT, XHCI_CODE_RESERVED,
            XHCI_CODE_ISOC_OVERRUN, XHCI_CODE_EVENT_LOST, XHCI_CODE_UNDEFINED,
            XHCI_CODE_INVALID_SID, XHCI_CODE_SEC_BW, XHCI_CODE_SPLITERR,
        );
        // A string for `%b`, not an integer.
        ours.push("XHCI_TRB_FLAGS_BITMASK");
        crate::reftest::assert_complete(&defs, "XHCI_", &ours);
        crate::reftest::assert_complete(&defs, "PCI_", &ours);
    }
}
/* </TESTS> */
