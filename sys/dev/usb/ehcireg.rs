/*	$OpenBSD: ehcireg.h,v 1.21 2016/10/02 06:36:39 kettenis Exp $ */
/*	$NetBSD: ehcireg.h,v 1.17 2004/06/23 06:45:56 mycroft Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2001, 2004 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Lennart Augustsson (lennart@augustsson.net).
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
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! The EHCI registers and the structures the controller shares with the driver in memory:
//! `<dev/usb/ehcireg.h>`.
//!
//! Upstream: sys/dev/usb/ehcireg.h @ 3ce1f3f79392
//!
//! The capability registers sit at the start of the memory BAR; the operational registers
//! follow at `EHCI_CAPLENGTH`. The periodic frame list, the queue heads (`struct ehci_qh`),
//! the queue element transfer descriptors (`struct ehci_qtd`) and the isochronous descriptors
//! (`struct ehci_itd`, `struct ehci_sitd`) live in DMA memory, in the controller's
//! little-endian layout, and every pointer between them is a 32-bit bus address with its
//! type in the low bits (`ehci_link_t`).
//!
//! ## Deviations
//! - Register offsets are `usize` (`bus_size_t`); register bits, link words and descriptor
//!   fields `u32`; the PCI configuration register numbers `i32` (`pci_conf_read`'s `int reg`).
//! - Function-like macros (`EHCI_PORTSC(n)`, `EHCI_QTD_GET_BYTES(x)`, ...) are `const fn`s
//!   with the macro's name in lower case (`docs/C_TO_RUST.md`).
//! - The descriptors are `#[repr(C)]` structures of `Cell<u32>` with the C's sizes pinned by
//!   compile-time checks. The driver keeps its software state next to them in the same DMA
//!   chunk (`ehcivar.rs`), so it reaches them as `&'static` and every access to a member is
//!   one volatile load or store through [`ehci_get`] and [`ehci_set`], which also do the
//!   `letoh32`/`htole32` conversions (`docs/C_TO_RUST.md`, a structure a chip and the driver
//!   share in DMA memory). The C marks only the isochronous descriptors' members `volatile`
//!   and relies on `usb_syncmem` for the others; here every access is volatile.

use core::cell::Cell;
use core::ptr;

use crate::machine::bus::BusSize;

/*** PCI config registers ***/

/// `PCI_CBMEM`: configuration base MEM.
pub const PCI_CBMEM: i32 = 0x10;

/// `PCI_INTERFACE_EHCI`.
pub const PCI_INTERFACE_EHCI: u32 = 0x20;

/// `PCI_USBREV`: RO USB protocol revision.
pub const PCI_USBREV: i32 = 0x60;
/// `PCI_USBREV_MASK`.
pub const PCI_USBREV_MASK: u32 = 0xff;
/// `PCI_USBREV_PRE_1_0`.
pub const PCI_USBREV_PRE_1_0: u32 = 0x00;
/// `PCI_USBREV_1_0`.
pub const PCI_USBREV_1_0: u32 = 0x10;
/// `PCI_USBREV_1_1`.
pub const PCI_USBREV_1_1: u32 = 0x11;
/// `PCI_USBREV_2_0`.
pub const PCI_USBREV_2_0: u32 = 0x20;

/// `PCI_EHCI_FLADJ`: RW Frame len adj, SOF=59488+6*fladj.
pub const PCI_EHCI_FLADJ: i32 = 0x61;

/// `PCI_EHCI_PORTWAKECAP`: RW Port wake caps (opt).
pub const PCI_EHCI_PORTWAKECAP: i32 = 0x62;

/* EHCI Extended Capabilities */
/// `EHCI_EC_LEGSUP`.
pub const EHCI_EC_LEGSUP: u32 = 0x01;

/// `EHCI_EECP_NEXT(x)`.
pub const fn ehci_eecp_next(x: u32) -> u32 {
    (x >> 8) & 0xff
}

/// `EHCI_EECP_ID(x)`.
pub const fn ehci_eecp_id(x: u32) -> u32 {
    x & 0xff
}

/// `EHCI_LEGSUP_LEGSUP`.
pub const EHCI_LEGSUP_LEGSUP: u32 = 0x00;
/// `EHCI_LEGSUP_OSOWNED`: OS owned semaphore.
pub const EHCI_LEGSUP_OSOWNED: u32 = 0x01000000;
/// `EHCI_LEGSUP_BIOSOWNED`: BIOS owned semaphore.
pub const EHCI_LEGSUP_BIOSOWNED: u32 = 0x00010000;
/// `PCI_LEGSUP_USBLEGCTLSTS`.
pub const PCI_LEGSUP_USBLEGCTLSTS: i32 = 0x04;

/*** EHCI capability registers ***/

/// `EHCI_CAPLENGTH`: RO Capability register length field.
pub const EHCI_CAPLENGTH: BusSize = 0x00;
/* reserved 0x01 */
/// `EHCI_HCIVERSION`: RO Interface version number.
pub const EHCI_HCIVERSION: BusSize = 0x02;

/// `EHCI_HCSPARAMS`: RO Structural parameters.
pub const EHCI_HCSPARAMS: BusSize = 0x04;

/// `EHCI_HCS_DEBUGPORT(x)`.
pub const fn ehci_hcs_debugport(x: u32) -> u32 {
    (x >> 20) & 0xf
}

/// `EHCI_HCS_P_INDICATOR(x)`.
pub const fn ehci_hcs_p_indicator(x: u32) -> u32 {
    x & 0x10000
}

/// `EHCI_HCS_N_CC(x)`: # of companion ctlrs.
pub const fn ehci_hcs_n_cc(x: u32) -> u32 {
    (x >> 12) & 0xf
}

/// `EHCI_HCS_N_PCC(x)`: # of ports per comp.
pub const fn ehci_hcs_n_pcc(x: u32) -> u32 {
    (x >> 8) & 0xf
}

/// `EHCI_HCS_PRR(x)`: port routing rules.
pub const fn ehci_hcs_prr(x: u32) -> u32 {
    x & 0x80
}

/// `EHCI_HCS_PPC(x)`: port power control.
pub const fn ehci_hcs_ppc(x: u32) -> u32 {
    x & 0x10
}

/// `EHCI_HCS_N_PORTS(x)`: # of ports.
pub const fn ehci_hcs_n_ports(x: u32) -> u32 {
    x & 0xf
}

/// `EHCI_HCCPARAMS`: RO Capability parameters.
pub const EHCI_HCCPARAMS: BusSize = 0x08;

/// `EHCI_HCC_EECP(x)`: extended ports caps.
pub const fn ehci_hcc_eecp(x: u32) -> u32 {
    (x >> 8) & 0xff
}

/// `EHCI_HCC_IST(x)`: isoc sched threshold.
pub const fn ehci_hcc_ist(x: u32) -> u32 {
    (x >> 4) & 0xf
}

/// `EHCI_HCC_ASPC(x)`: async sched park cap.
pub const fn ehci_hcc_aspc(x: u32) -> u32 {
    x & 0x4
}

/// `EHCI_HCC_PFLF(x)`: prog frame list flag.
pub const fn ehci_hcc_pflf(x: u32) -> u32 {
    x & 0x2
}

/// `EHCI_HCC_64BIT(x)`: 64 bit address cap.
pub const fn ehci_hcc_64bit(x: u32) -> u32 {
    x & 0x1
}

/// `EHCI_HCSP_PORTROUTE`: RO Companion port route description.
pub const EHCI_HCSP_PORTROUTE: BusSize = 0x0c;

/* EHCI operational registers.  Offset given by EHCI_CAPLENGTH register */
/// `EHCI_USBCMD`: RO, RW, WO Command register.
pub const EHCI_USBCMD: BusSize = 0x00;
/// `EHCI_CMD_ITC_M`: RW interrupt threshold ctrl.
pub const EHCI_CMD_ITC_M: u32 = 0x00ff0000;
/// `EHCI_CMD_ITC_1`.
pub const EHCI_CMD_ITC_1: u32 = 0x00010000;
/// `EHCI_CMD_ITC_2`.
pub const EHCI_CMD_ITC_2: u32 = 0x00020000;
/// `EHCI_CMD_ITC_4`.
pub const EHCI_CMD_ITC_4: u32 = 0x00040000;
/// `EHCI_CMD_ITC_8`.
pub const EHCI_CMD_ITC_8: u32 = 0x00080000;
/// `EHCI_CMD_ITC_16`.
pub const EHCI_CMD_ITC_16: u32 = 0x00100000;
/// `EHCI_CMD_ITC_32`.
pub const EHCI_CMD_ITC_32: u32 = 0x00200000;
/// `EHCI_CMD_ITC_64`.
pub const EHCI_CMD_ITC_64: u32 = 0x00400000;
/// `EHCI_CMD_ASPME`: RW/RO async park enable.
pub const EHCI_CMD_ASPME: u32 = 0x00000800;
/// `EHCI_CMD_ASPMC`: RW/RO async park count.
pub const EHCI_CMD_ASPMC: u32 = 0x00000300;
/// `EHCI_CMD_LHCR`: RW light host ctrl reset.
pub const EHCI_CMD_LHCR: u32 = 0x00000080;
/// `EHCI_CMD_IAAD`: RW intr on async adv door bell.
pub const EHCI_CMD_IAAD: u32 = 0x00000040;
/// `EHCI_CMD_ASE`: RW async sched enable.
pub const EHCI_CMD_ASE: u32 = 0x00000020;
/// `EHCI_CMD_PSE`: RW periodic sched enable.
pub const EHCI_CMD_PSE: u32 = 0x00000010;
/// `EHCI_CMD_FLS_M`: RW/RO frame list size.
pub const EHCI_CMD_FLS_M: u32 = 0x0000000c;

/// `EHCI_CMD_FLS(x)`: RW/RO frame list size.
pub const fn ehci_cmd_fls(x: u32) -> u32 {
    (x >> 2) & 3
}

/// `EHCI_CMD_HCRESET`: RW reset.
pub const EHCI_CMD_HCRESET: u32 = 0x00000002;
/// `EHCI_CMD_RS`: RW run/stop.
pub const EHCI_CMD_RS: u32 = 0x00000001;

/// `EHCI_USBSTS`: RO, RW, RWC Status register.
pub const EHCI_USBSTS: BusSize = 0x04;
/// `EHCI_STS_ASS`: RO async sched status.
pub const EHCI_STS_ASS: u32 = 0x00008000;
/// `EHCI_STS_PSS`: RO periodic sched status.
pub const EHCI_STS_PSS: u32 = 0x00004000;
/// `EHCI_STS_REC`: RO reclamation.
pub const EHCI_STS_REC: u32 = 0x00002000;
/// `EHCI_STS_HCH`: RO host controller halted.
pub const EHCI_STS_HCH: u32 = 0x00001000;
/// `EHCI_STS_IAA`: RWC interrupt on async adv.
pub const EHCI_STS_IAA: u32 = 0x00000020;
/// `EHCI_STS_HSE`: RWC host system error.
pub const EHCI_STS_HSE: u32 = 0x00000010;
/// `EHCI_STS_FLR`: RWC frame list rollover.
pub const EHCI_STS_FLR: u32 = 0x00000008;
/// `EHCI_STS_PCD`: RWC port change detect.
pub const EHCI_STS_PCD: u32 = 0x00000004;
/// `EHCI_STS_ERRINT`: RWC error interrupt.
pub const EHCI_STS_ERRINT: u32 = 0x00000002;
/// `EHCI_STS_INT`: RWC interrupt.
pub const EHCI_STS_INT: u32 = 0x00000001;

/// `EHCI_STS_INTRS(x)`.
pub const fn ehci_sts_intrs(x: u32) -> u32 {
    x & 0x3f
}

/// `EHCI_NORMAL_INTRS`.
pub const EHCI_NORMAL_INTRS: u32 =
    EHCI_STS_IAA | EHCI_STS_HSE | EHCI_STS_PCD | EHCI_STS_ERRINT | EHCI_STS_INT;

/// `EHCI_USBINTR`: RW Interrupt register.
pub const EHCI_USBINTR: BusSize = 0x08;
/// `EHCI_INTR_IAAE`: interrupt on async advance ena.
pub const EHCI_INTR_IAAE: u32 = 0x00000020;
/// `EHCI_INTR_HSEE`: host system error ena.
pub const EHCI_INTR_HSEE: u32 = 0x00000010;
/// `EHCI_INTR_FLRE`: frame list rollover ena.
pub const EHCI_INTR_FLRE: u32 = 0x00000008;
/// `EHCI_INTR_PCIE`: port change ena.
pub const EHCI_INTR_PCIE: u32 = 0x00000004;
/// `EHCI_INTR_UEIE`: USB error intr ena.
pub const EHCI_INTR_UEIE: u32 = 0x00000002;
/// `EHCI_INTR_UIE`: USB intr ena.
pub const EHCI_INTR_UIE: u32 = 0x00000001;

/// `EHCI_FRINDEX`: RW Frame Index register.
pub const EHCI_FRINDEX: BusSize = 0x0c;

/// `EHCI_CTRLDSSEGMENT`: RW Control Data Structure Segment.
pub const EHCI_CTRLDSSEGMENT: BusSize = 0x10;

/// `EHCI_PERIODICLISTBASE`: RW Periodic List Base.
pub const EHCI_PERIODICLISTBASE: BusSize = 0x14;
/// `EHCI_ASYNCLISTADDR`: RW Async List Base.
pub const EHCI_ASYNCLISTADDR: BusSize = 0x18;

/// `EHCI_CONFIGFLAG`: RW Configure Flag register.
pub const EHCI_CONFIGFLAG: BusSize = 0x40;
/// `EHCI_CONF_CF`: RW configure flag.
pub const EHCI_CONF_CF: u32 = 0x00000001;

/// `EHCI_PORTSC(n)`: RO, RW, RWC Port Status reg.
pub const fn ehci_portsc(n: usize) -> BusSize {
    0x40 + 4 * n
}

/// `EHCI_PS_WKOC_E`: RW wake on over current ena.
pub const EHCI_PS_WKOC_E: u32 = 0x00400000;
/// `EHCI_PS_WKDSCNNT_E`: RW wake on disconnect ena.
pub const EHCI_PS_WKDSCNNT_E: u32 = 0x00200000;
/// `EHCI_PS_WKCNNT_E`: RW wake on connect ena.
pub const EHCI_PS_WKCNNT_E: u32 = 0x00100000;
/// `EHCI_PS_PTC`: RW port test control.
pub const EHCI_PS_PTC: u32 = 0x000f0000;
/// `EHCI_PS_PIC`: RW port indicator control.
pub const EHCI_PS_PIC: u32 = 0x0000c000;
/// `EHCI_PS_PO`: RW port owner.
pub const EHCI_PS_PO: u32 = 0x00002000;
/// `EHCI_PS_PP`: RW,RO port power.
pub const EHCI_PS_PP: u32 = 0x00001000;
/// `EHCI_PS_LS`: RO line status.
pub const EHCI_PS_LS: u32 = 0x00000c00;

/// `EHCI_PS_IS_LOWSPEED(x)`.
pub const fn ehci_ps_is_lowspeed(x: u32) -> bool {
    (x & EHCI_PS_LS) == 0x00000400
}

/// `EHCI_PS_PR`: RW port reset.
pub const EHCI_PS_PR: u32 = 0x00000100;
/// `EHCI_PS_SUSP`: RW suspend.
pub const EHCI_PS_SUSP: u32 = 0x00000080;
/// `EHCI_PS_FPR`: RW force port resume.
pub const EHCI_PS_FPR: u32 = 0x00000040;
/// `EHCI_PS_OCC`: RWC over current change.
pub const EHCI_PS_OCC: u32 = 0x00000020;
/// `EHCI_PS_OCA`: RO over current active.
pub const EHCI_PS_OCA: u32 = 0x00000010;
/// `EHCI_PS_PEC`: RWC port enable change.
pub const EHCI_PS_PEC: u32 = 0x00000008;
/// `EHCI_PS_PE`: RW port enable.
pub const EHCI_PS_PE: u32 = 0x00000004;
/// `EHCI_PS_CSC`: RWC connect status change.
pub const EHCI_PS_CSC: u32 = 0x00000002;
/// `EHCI_PS_CS`: RO connect status.
pub const EHCI_PS_CS: u32 = 0x00000001;
/// `EHCI_PS_CLEAR`.
pub const EHCI_PS_CLEAR: u32 = EHCI_PS_OCC | EHCI_PS_PEC | EHCI_PS_CSC;

/// `EHCI_PORT_RESET_COMPLETE`: ms.
pub const EHCI_PORT_RESET_COMPLETE: u32 = 2;

/* Nonstandard register to set controller mode. */
/// `EHCI_USBMODE`.
pub const EHCI_USBMODE: BusSize = 0x68;
/// `EHCI_USBMODE_CM_M`.
pub const EHCI_USBMODE_CM_M: u32 = 0x00000003;
/// `EHCI_USBMODE_CM_IDLE`.
pub const EHCI_USBMODE_CM_IDLE: u32 = 0x00000000;
/// `EHCI_USBMODE_CM_DEVICE`.
pub const EHCI_USBMODE_CM_DEVICE: u32 = 0x00000002;
/// `EHCI_USBMODE_CM_HOST`.
pub const EHCI_USBMODE_CM_HOST: u32 = 0x00000003;

/// `EHCI_FLALIGN_ALIGN`.
pub const EHCI_FLALIGN_ALIGN: usize = 0x1000;

/* No data structure may cross a page boundary. */
/// `EHCI_PAGE_SIZE`.
pub const EHCI_PAGE_SIZE: u32 = 0x1000;

/// `EHCI_PAGE(x)`.
pub const fn ehci_page(x: u32) -> u32 {
    x & !0xfff
}

/// `EHCI_PAGE_OFFSET(x)`.
pub const fn ehci_page_offset(x: u32) -> u32 {
    x & 0xfff
}

/// `ehci_link_t`.
pub type EhciLinkT = u32;
/// `EHCI_LINK_TERMINATE`.
pub const EHCI_LINK_TERMINATE: u32 = 0x00000001;

/// `EHCI_LINK_TYPE(x)`.
pub const fn ehci_link_type(x: u32) -> u32 {
    x & 0x00000006
}

/// `EHCI_LINK_ITD`.
pub const EHCI_LINK_ITD: u32 = 0x0;
/// `EHCI_LINK_QH`.
pub const EHCI_LINK_QH: u32 = 0x2;
/// `EHCI_LINK_SITD`.
pub const EHCI_LINK_SITD: u32 = 0x4;
/// `EHCI_LINK_FSTN`.
pub const EHCI_LINK_FSTN: u32 = 0x6;

/// `EHCI_LINK_ADDR(x)`.
pub const fn ehci_link_addr(x: u32) -> u32 {
    x & !0x1f
}

/// `ehci_physaddr_t`.
pub type EhciPhysaddrT = u32;
/// `ehci_isoc_trans_t`.
pub type EhciIsocTransT = u32;
/// `ehci_isoc_bufr_ptr_t`.
pub type EhciIsocBufrPtrT = u32;
/// `EHCI_BUFPTR_MASK`.
pub const EHCI_BUFPTR_MASK: u32 = 0xfffff000;

/* Isochronous Transfer Descriptor */
/// `EHCI_ITD_NTRANS`.
pub const EHCI_ITD_NTRANS: usize = 8;
/// `EHCI_ITD_NBUFFERS`.
pub const EHCI_ITD_NBUFFERS: usize = 7;

/// `struct ehci_itd`: an isochronous transfer descriptor (high speed).
#[repr(C)]
pub struct EhciItd {
    /// `itd_next`.
    pub itd_next: Cell<EhciLinkT>,
    /// `itd_ctl`: one transaction per microframe.
    pub itd_ctl: [Cell<EhciIsocTransT>; 8],
    /// `itd_bufr`: the buffer pages; the low bits of the first three carry the endpoint.
    pub itd_bufr: [Cell<EhciIsocBufrPtrT>; 7],
    /// `itd_bufr_hi`: 64-bit buffer pointers.
    pub itd_bufr_hi: [Cell<EhciIsocBufrPtrT>; 7],
}

/// `EHCI_ITD_GET_STATUS(x)`.
pub const fn ehci_itd_get_status(x: u32) -> u32 {
    (x >> 28) & 0xf
}

/// `EHCI_ITD_SET_STATUS(x)`.
pub const fn ehci_itd_set_status(x: u32) -> u32 {
    (x & 0xf) << 28
}

/// `EHCI_ITD_ACTIVE`.
pub const EHCI_ITD_ACTIVE: u32 = 0x80000000;
/// `EHCI_ITD_BUF_ERR`.
pub const EHCI_ITD_BUF_ERR: u32 = 0x40000000;
/// `EHCI_ITD_BABBLE`.
pub const EHCI_ITD_BABBLE: u32 = 0x20000000;
/// `EHCI_ITD_ERROR`.
pub const EHCI_ITD_ERROR: u32 = 0x10000000;

/// `EHCI_ITD_GET_LEN(x)`.
pub const fn ehci_itd_get_len(x: u32) -> u32 {
    (x >> 16) & 0xfff
}

/// `EHCI_ITD_SET_LEN(x)`.
pub const fn ehci_itd_set_len(x: u32) -> u32 {
    (x & 0xfff) << 16
}

/// `EHCI_ITD_IOC`.
pub const EHCI_ITD_IOC: u32 = 0x8000;

/// `EHCI_ITD_GET_IOC(x)`.
pub const fn ehci_itd_get_ioc(x: u32) -> u32 {
    (x >> 15) & 1
}

/// `EHCI_ITD_SET_IOC(x)`.
pub const fn ehci_itd_set_ioc(x: u32) -> u32 {
    (x << 15) & EHCI_ITD_IOC
}

/// `EHCI_ITD_GET_PG(x)`.
pub const fn ehci_itd_get_pg(x: u32) -> u32 {
    (x >> 12) & 0x7
}

/// `EHCI_ITD_SET_PG(x)`.
pub const fn ehci_itd_set_pg(x: u32) -> u32 {
    (x & 0x7) << 12
}

/// `EHCI_ITD_GET_OFFS(x)`.
pub const fn ehci_itd_get_offs(x: u32) -> u32 {
    x & 0xfff
}

/// `EHCI_ITD_SET_OFFS(x)`.
pub const fn ehci_itd_set_offs(x: u32) -> u32 {
    x & 0xfff
}

/// `EHCI_ITD_GET_ENDPT(x)`.
pub const fn ehci_itd_get_endpt(x: u32) -> u32 {
    (x >> 8) & 0xf
}

/// `EHCI_ITD_SET_ENDPT(x)`.
pub const fn ehci_itd_set_endpt(x: u32) -> u32 {
    (x & 0xf) << 8
}

/// `EHCI_ITD_GET_DADDR(x)`.
pub const fn ehci_itd_get_daddr(x: u32) -> u32 {
    x & 0x7f
}

/// `EHCI_ITD_SET_DADDR(x)`.
pub const fn ehci_itd_set_daddr(x: u32) -> u32 {
    x & 0x7f
}

/// `EHCI_ITD_GET_DIR(x)`.
pub const fn ehci_itd_get_dir(x: u32) -> u32 {
    (x >> 11) & 1
}

/// `EHCI_ITD_SET_DIR(x)`.
pub const fn ehci_itd_set_dir(x: u32) -> u32 {
    (x & 1) << 11
}

/// `EHCI_ITD_GET_MAXPKT(x)`.
pub const fn ehci_itd_get_maxpkt(x: u32) -> u32 {
    x & 0x7ff
}

/// `EHCI_ITD_SET_MAXPKT(x)`.
pub const fn ehci_itd_set_maxpkt(x: u32) -> u32 {
    x & 0x7ff
}

/// `EHCI_ITD_GET_MULTI(x)`.
pub const fn ehci_itd_get_multi(x: u32) -> u32 {
    x & 0x3
}

/// `EHCI_ITD_SET_MULTI(x)`.
pub const fn ehci_itd_set_multi(x: u32) -> u32 {
    x & 0x3
}

/// `EHCI_ITD_ALIGN`.
pub const EHCI_ITD_ALIGN: usize = 32;

/// `struct ehci_sitd`: a split transaction isochronous transfer descriptor.
#[repr(C)]
pub struct EhciSitd {
    /// `sitd_next`.
    pub sitd_next: Cell<EhciLinkT>,
    /// `sitd_endp`.
    pub sitd_endp: Cell<u32>,
    /// `sitd_sched`.
    pub sitd_sched: Cell<u32>,
    /// `sitd_trans`.
    pub sitd_trans: Cell<u32>,
    /// `sitd_bufr`.
    pub sitd_bufr: [Cell<EhciPhysaddrT>; 2],
    /// `sitd_back`.
    pub sitd_back: Cell<EhciLinkT>,
    /// `sitd_bufr_hi`: 64bit.
    pub sitd_bufr_hi: [Cell<EhciPhysaddrT>; 2],
}

/// `EHCI_SITD_GET_ADDR(x)`: endpoint addr.
pub const fn ehci_sitd_get_addr(x: u32) -> u32 {
    x & 0x7f
}

/// `EHCI_SITD_SET_ADDR(x)`.
pub const fn ehci_sitd_set_addr(x: u32) -> u32 {
    x
}

/// `EHCI_SITD_GET_ENDPT(x)`: endpoint no.
pub const fn ehci_sitd_get_endpt(x: u32) -> u32 {
    (x >> 8) & 0xf
}

/// `EHCI_SITD_SET_ENDPT(x)`.
pub const fn ehci_sitd_set_endpt(x: u32) -> u32 {
    x << 8
}

/// `EHCI_SITD_GET_HUBA(x)`: hub address.
pub const fn ehci_sitd_get_huba(x: u32) -> u32 {
    (x >> 16) & 0x7f
}

/// `EHCI_SITD_SET_HUBA(x)`.
pub const fn ehci_sitd_set_huba(x: u32) -> u32 {
    x << 16
}

/// `EHCI_SITD_GET_PORT(x)`: hub port.
pub const fn ehci_sitd_get_port(x: u32) -> u32 {
    (x >> 23) & 0x7f
}

/// `EHCI_SITD_SET_PORT(x)`.
pub const fn ehci_sitd_set_port(x: u32) -> u32 {
    x << 23
}

/// `EHCI_SITD_GET_DIR(x)`: direction.
pub const fn ehci_sitd_get_dir(x: u32) -> u32 {
    (x >> 31) & 0x1
}

/// `EHCI_SITD_SET_DIR(x)`.
pub const fn ehci_sitd_set_dir(x: u32) -> u32 {
    x << 31
}

/// `EHCI_SITD_GET_SMASK(x)`: intr sched mask.
pub const fn ehci_sitd_get_smask(x: u32) -> u32 {
    x & 0xff
}

/// `EHCI_SITD_SET_SMASK(x)`.
pub const fn ehci_sitd_set_smask(x: u32) -> u32 {
    x
}

/// `EHCI_SITD_GET_CMASK(x)`: split completion mask.
pub const fn ehci_sitd_get_cmask(x: u32) -> u32 {
    (x >> 8) & 0xff
}

/// `EHCI_SITD_SET_CMASK(x)`.
pub const fn ehci_sitd_set_cmask(x: u32) -> u32 {
    x << 8
}

/// `EHCI_SITD_IOC`.
pub const EHCI_SITD_IOC: u32 = 0x80000000;
/// `EHCI_SITD_ACTIVE`.
pub const EHCI_SITD_ACTIVE: u32 = 0x80;
/// `EHCI_SITD_ERR`.
pub const EHCI_SITD_ERR: u32 = 0x40;
/// `EHCI_SITD_BUFERR`.
pub const EHCI_SITD_BUFERR: u32 = 0x20;
/// `EHCI_SITD_BABBLE`.
pub const EHCI_SITD_BABBLE: u32 = 0x10;
/// `EHCI_SITD_XACTERR`.
pub const EHCI_SITD_XACTERR: u32 = 0x08;
/// `EHCI_SITD_MISSEDMICRO`.
pub const EHCI_SITD_MISSEDMICRO: u32 = 0x04;
/// `EHCI_SITD_SPLITXSTATE`.
pub const EHCI_SITD_SPLITXSTATE: u32 = 0x02;

/// `EHCI_SITD_GET_LEN(x)`: bytes to transfer.
pub const fn ehci_sitd_get_len(x: u32) -> u32 {
    (x >> 16) & 0x3ff
}

/// `EHCI_SITD_SET_LEN(x)`.
pub const fn ehci_sitd_set_len(x: u32) -> u32 {
    (x & 0x3ff) << 16
}

/// `EHCI_SITD_GET_PG(x)`: buffer page.
pub const fn ehci_sitd_get_pg(x: u32) -> u32 {
    (x >> 30) & 0x1
}

/// `EHCI_SITD_SET_PG(x)`.
pub const fn ehci_sitd_set_pg(x: u32) -> u32 {
    x << 30
}

/// `EHCI_SITD_GET_TCOUNT(x)`: transaction count.
pub const fn ehci_sitd_get_tcount(x: u32) -> u32 {
    x & 0x7
}

/// `EHCI_SITD_SET_TCOUNT(x)`.
pub const fn ehci_sitd_set_tcount(x: u32) -> u32 {
    x
}

/// `EHCI_SITD_GET_TP(x)`: transaction position.
pub const fn ehci_sitd_get_tp(x: u32) -> u32 {
    (x >> 3) & 0x3
}

/// `EHCI_SITD_SET_TP(x)`.
pub const fn ehci_sitd_set_tp(x: u32) -> u32 {
    x << 3
}

/// `EHCI_SITD_TP_ALL`.
pub const EHCI_SITD_TP_ALL: u32 = 0x0;
/// `EHCI_SITD_TP_BEGIN`.
pub const EHCI_SITD_TP_BEGIN: u32 = 0x1;
/// `EHCI_SITD_TP_MIDDLE`.
pub const EHCI_SITD_TP_MIDDLE: u32 = 0x2;
/// `EHCI_SITD_TP_END`.
pub const EHCI_SITD_TP_END: u32 = 0x3;

/// `EHCI_SITD_ALIGN`.
pub const EHCI_SITD_ALIGN: usize = 32;

/* Queue Element Transfer Descriptor */
/// `EHCI_QTD_NBUFFERS`.
pub const EHCI_QTD_NBUFFERS: usize = 5;

/// `struct ehci_qtd`: a queue element transfer descriptor.
#[repr(C)]
pub struct EhciQtd {
    /// `qtd_next`.
    pub qtd_next: Cell<EhciLinkT>,
    /// `qtd_altnext`.
    pub qtd_altnext: Cell<EhciLinkT>,
    /// `qtd_status`.
    pub qtd_status: Cell<u32>,
    /// `qtd_buffer`.
    pub qtd_buffer: [Cell<EhciPhysaddrT>; EHCI_QTD_NBUFFERS],
    /// `qtd_buffer_hi`.
    pub qtd_buffer_hi: [Cell<EhciPhysaddrT>; EHCI_QTD_NBUFFERS],
}

/// `EHCI_QTD_GET_STATUS(x)`.
pub const fn ehci_qtd_get_status(x: u32) -> u32 {
    x & 0xff
}

/// `EHCI_QTD_SET_STATUS(x)`.
pub const fn ehci_qtd_set_status(x: u32) -> u32 {
    x
}

/// `EHCI_QTD_ACTIVE`.
pub const EHCI_QTD_ACTIVE: u32 = 0x80;
/// `EHCI_QTD_HALTED`.
pub const EHCI_QTD_HALTED: u32 = 0x40;
/// `EHCI_QTD_BUFERR`.
pub const EHCI_QTD_BUFERR: u32 = 0x20;
/// `EHCI_QTD_BABBLE`.
pub const EHCI_QTD_BABBLE: u32 = 0x10;
/// `EHCI_QTD_XACTERR`.
pub const EHCI_QTD_XACTERR: u32 = 0x08;
/// `EHCI_QTD_MISSEDMICRO`.
pub const EHCI_QTD_MISSEDMICRO: u32 = 0x04;
/// `EHCI_QTD_SPLITXSTATE`.
pub const EHCI_QTD_SPLITXSTATE: u32 = 0x02;
/// `EHCI_QTD_PINGSTATE`.
pub const EHCI_QTD_PINGSTATE: u32 = 0x01;
/// `EHCI_QTD_STATERRS`.
pub const EHCI_QTD_STATERRS: u32 = 0x7c;

/// `EHCI_QTD_GET_PID(x)`.
pub const fn ehci_qtd_get_pid(x: u32) -> u32 {
    (x >> 8) & 0x3
}

/// `EHCI_QTD_SET_PID(x)`.
pub const fn ehci_qtd_set_pid(x: u32) -> u32 {
    x << 8
}

/// `EHCI_QTD_PID_OUT`.
pub const EHCI_QTD_PID_OUT: u32 = 0x0;
/// `EHCI_QTD_PID_IN`.
pub const EHCI_QTD_PID_IN: u32 = 0x1;
/// `EHCI_QTD_PID_SETUP`.
pub const EHCI_QTD_PID_SETUP: u32 = 0x2;

/// `EHCI_QTD_GET_CERR(x)`.
pub const fn ehci_qtd_get_cerr(x: u32) -> u32 {
    (x >> 10) & 0x3
}

/// `EHCI_QTD_SET_CERR(x)`.
pub const fn ehci_qtd_set_cerr(x: u32) -> u32 {
    x << 10
}

/// `EHCI_QTD_GET_C_PAGE(x)`.
pub const fn ehci_qtd_get_c_page(x: u32) -> u32 {
    (x >> 12) & 0x7
}

/// `EHCI_QTD_SET_C_PAGE(x)`.
pub const fn ehci_qtd_set_c_page(x: u32) -> u32 {
    x << 12
}

/// `EHCI_QTD_GET_IOC(x)`.
pub const fn ehci_qtd_get_ioc(x: u32) -> u32 {
    (x >> 15) & 0x1
}

/// `EHCI_QTD_IOC`.
pub const EHCI_QTD_IOC: u32 = 0x00008000;

/// `EHCI_QTD_GET_BYTES(x)`.
pub const fn ehci_qtd_get_bytes(x: u32) -> u32 {
    (x >> 16) & 0x7fff
}

/// `EHCI_QTD_SET_BYTES(x)`.
pub const fn ehci_qtd_set_bytes(x: u32) -> u32 {
    x << 16
}

/// `EHCI_QTD_GET_TOGGLE(x)`.
pub const fn ehci_qtd_get_toggle(x: u32) -> u32 {
    (x >> 31) & 0x1
}

/// `EHCI_QTD_SET_TOGGLE(x)`.
pub const fn ehci_qtd_set_toggle(x: u32) -> u32 {
    x << 31
}

/// `EHCI_QTD_TOGGLE_MASK`.
pub const EHCI_QTD_TOGGLE_MASK: u32 = 0x80000000;

/// `EHCI_QTD_ALIGN`.
pub const EHCI_QTD_ALIGN: usize = 32;

/// `struct ehci_qh`: a queue head, with its overlay qTD.
#[repr(C)]
pub struct EhciQh {
    /// `qh_link`.
    pub qh_link: Cell<EhciLinkT>,
    /// `qh_endp`.
    pub qh_endp: Cell<u32>,
    /// `qh_endphub`.
    pub qh_endphub: Cell<u32>,
    /// `qh_curqtd`.
    pub qh_curqtd: Cell<EhciLinkT>,
    /// `qh_qtd`: the overlay.
    pub qh_qtd: EhciQtd,
}

/// `EHCI_QH_GET_ADDR(x)`: endpoint addr.
pub const fn ehci_qh_get_addr(x: u32) -> u32 {
    x & 0x7f
}

/// `EHCI_QH_SET_ADDR(x)`.
pub const fn ehci_qh_set_addr(x: u32) -> u32 {
    x
}

/// `EHCI_QH_ADDRMASK`.
pub const EHCI_QH_ADDRMASK: u32 = 0x0000007f;

/// `EHCI_QH_GET_INACT(x)`: inactivate on next.
pub const fn ehci_qh_get_inact(x: u32) -> u32 {
    (x >> 7) & 0x01
}

/// `EHCI_QH_INACT`.
pub const EHCI_QH_INACT: u32 = 0x00000080;

/// `EHCI_QH_GET_ENDPT(x)`: endpoint no.
pub const fn ehci_qh_get_endpt(x: u32) -> u32 {
    (x >> 8) & 0x0f
}

/// `EHCI_QH_SET_ENDPT(x)`.
pub const fn ehci_qh_set_endpt(x: u32) -> u32 {
    x << 8
}

/// `EHCI_QH_GET_EPS(x)`: endpoint speed.
pub const fn ehci_qh_get_eps(x: u32) -> u32 {
    (x >> 12) & 0x03
}

/// `EHCI_QH_SET_EPS(x)`.
pub const fn ehci_qh_set_eps(x: u32) -> u32 {
    x << 12
}

/// `EHCI_QH_SPEED_FULL`.
pub const EHCI_QH_SPEED_FULL: u32 = 0x0;
/// `EHCI_QH_SPEED_LOW`.
pub const EHCI_QH_SPEED_LOW: u32 = 0x1;
/// `EHCI_QH_SPEED_HIGH`.
pub const EHCI_QH_SPEED_HIGH: u32 = 0x2;

/// `EHCI_QH_GET_DTC(x)`: data toggle control.
pub const fn ehci_qh_get_dtc(x: u32) -> u32 {
    (x >> 14) & 0x01
}

/// `EHCI_QH_DTC`.
pub const EHCI_QH_DTC: u32 = 0x00004000;

/// `EHCI_QH_GET_HRECL(x)`: head of reclamation.
pub const fn ehci_qh_get_hrecl(x: u32) -> u32 {
    (x >> 15) & 0x01
}

/// `EHCI_QH_HRECL`.
pub const EHCI_QH_HRECL: u32 = 0x00008000;

/// `EHCI_QH_GET_MPL(x)`: max packet len.
pub const fn ehci_qh_get_mpl(x: u32) -> u32 {
    (x >> 16) & 0x7ff
}

/// `EHCI_QH_SET_MPL(x)`.
pub const fn ehci_qh_set_mpl(x: u32) -> u32 {
    x << 16
}

/// `EHCI_QH_MPLMASK`.
pub const EHCI_QH_MPLMASK: u32 = 0x07ff0000;

/// `EHCI_QH_GET_CTL(x)`: control endpoint.
pub const fn ehci_qh_get_ctl(x: u32) -> u32 {
    (x >> 27) & 0x01
}

/// `EHCI_QH_CTL`.
pub const EHCI_QH_CTL: u32 = 0x08000000;

/// `EHCI_QH_GET_NRL(x)`: NAK reload.
pub const fn ehci_qh_get_nrl(x: u32) -> u32 {
    (x >> 28) & 0x0f
}

/// `EHCI_QH_SET_NRL(x)`.
pub const fn ehci_qh_set_nrl(x: u32) -> u32 {
    x << 28
}

/// `EHCI_QH_GET_SMASK(x)`: intr sched mask.
pub const fn ehci_qh_get_smask(x: u32) -> u32 {
    x & 0xff
}

/// `EHCI_QH_SET_SMASK(x)`.
pub const fn ehci_qh_set_smask(x: u32) -> u32 {
    x
}

/// `EHCI_QH_GET_CMASK(x)`: split completion mask.
pub const fn ehci_qh_get_cmask(x: u32) -> u32 {
    (x >> 8) & 0xff
}

/// `EHCI_QH_SET_CMASK(x)`.
pub const fn ehci_qh_set_cmask(x: u32) -> u32 {
    x << 8
}

/// `EHCI_QH_GET_HUBA(x)`: hub address.
pub const fn ehci_qh_get_huba(x: u32) -> u32 {
    (x >> 16) & 0x7f
}

/// `EHCI_QH_SET_HUBA(x)`.
pub const fn ehci_qh_set_huba(x: u32) -> u32 {
    x << 16
}

/// `EHCI_QH_GET_PORT(x)`: hub port.
pub const fn ehci_qh_get_port(x: u32) -> u32 {
    (x >> 23) & 0x7f
}

/// `EHCI_QH_SET_PORT(x)`.
pub const fn ehci_qh_set_port(x: u32) -> u32 {
    x << 23
}

/// `EHCI_QH_GET_MULT(x)`: pipe multiplier.
pub const fn ehci_qh_get_mult(x: u32) -> u32 {
    (x >> 30) & 0x03
}

/// `EHCI_QH_SET_MULT(x)`.
pub const fn ehci_qh_set_mult(x: u32) -> u32 {
    x << 30
}

/// `EHCI_QH_ALIGN`.
pub const EHCI_QH_ALIGN: usize = 32;

/// `struct ehci_fstn`: a periodic frame span traversal node.
#[repr(C)]
pub struct EhciFstn {
    /// `fstn_link`.
    pub fstn_link: Cell<EhciLinkT>,
    /// `fstn_back`.
    pub fstn_back: Cell<EhciLinkT>,
}

/// `EHCI_FSTN_ALIGN`.
pub const EHCI_FSTN_ALIGN: usize = 32;

/// `letoh32(member)`: one volatile load of a descriptor member in DMA memory, converted from
/// the controller's little-endian layout.
#[inline]
pub fn ehci_get(c: &Cell<u32>) -> u32 {
    // SAFETY: the cell's own four bytes, aligned (`#[repr(C)]` at a 32-byte aligned address);
    // the controller writes the memory, so the load is volatile; any bits are a `u32`.
    u32::from_le(unsafe { ptr::read_volatile(c.as_ptr()) })
}

/// `member = htole32(v)`: one volatile store of a descriptor member in DMA memory.
#[inline]
pub fn ehci_set(c: &Cell<u32>, v: u32) {
    // SAFETY: the cell's own four bytes, aligned; a `Cell` may be written through a shared
    // reference, and the controller reads the memory, so the store is volatile.
    unsafe { ptr::write_volatile(c.as_ptr(), v.to_le()) }
}

const _: () = {
    assert!(size_of::<EhciItd>() == 92);
    assert!(size_of::<EhciSitd>() == 36);
    assert!(size_of::<EhciQtd>() == 52);
    assert!(size_of::<EhciQh>() == 68);
    assert!(size_of::<EhciFstn>() == 8);
    assert!(core::mem::offset_of!(EhciQh, qh_qtd) == 16);
    assert!(core::mem::offset_of!(EhciItd, itd_bufr) == 36);
    assert!(core::mem::offset_of!(EhciSitd, sitd_back) == 24);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_macros() {
        assert_eq!(ehci_portsc(1), 0x44);
        assert_eq!(ehci_portsc(6), 0x58);
        // QEMU's usb-ehci: 6 ports, no companions, no port power control.
        let hcs = 0x0000_0006;
        assert_eq!(ehci_hcs_n_ports(hcs), 6);
        assert_eq!(ehci_hcs_ppc(hcs), 0);
        assert_eq!(ehci_hcs_n_cc(0x3216), 3);
        assert_eq!(ehci_hcs_n_pcc(0x3216), 2);
        assert_ne!(ehci_hcs_ppc(0x3216), 0);
        assert_eq!(ehci_hcc_eecp(0x0000_a012), 0xa0);
        assert_eq!(ehci_hcc_ist(0x0000_a012), 1);
        assert_eq!(ehci_hcc_64bit(0x0000_a013), 1);
        assert_eq!(ehci_cmd_fls(0x0008_0b08), 2);
        assert_eq!(ehci_sts_intrs(0x0000_f03f), 0x3f);
        assert_eq!(ehci_eecp_next(0x0001_6801), 0x68);
        assert_eq!(ehci_eecp_id(0x0001_6801), EHCI_EC_LEGSUP);
        assert!(ehci_ps_is_lowspeed(0x0000_0401));
        assert!(!ehci_ps_is_lowspeed(0x0000_0801));
    }

    #[test]
    fn qtd_and_qh_fields() {
        let st = EHCI_QTD_ACTIVE
            | ehci_qtd_set_pid(EHCI_QTD_PID_IN)
            | ehci_qtd_set_cerr(3)
            | ehci_qtd_set_toggle(1)
            | ehci_qtd_set_bytes(512)
            | EHCI_QTD_IOC;
        assert_eq!(ehci_qtd_get_status(st), EHCI_QTD_ACTIVE);
        assert_eq!(ehci_qtd_get_pid(st), EHCI_QTD_PID_IN);
        assert_eq!(ehci_qtd_get_cerr(st), 3);
        assert_eq!(ehci_qtd_get_toggle(st), 1);
        assert_eq!(ehci_qtd_get_bytes(st), 512);
        assert_eq!(ehci_qtd_get_ioc(st), 1);
        assert_eq!(st & EHCI_QTD_TOGGLE_MASK, EHCI_QTD_TOGGLE_MASK);

        let endp = ehci_qh_set_addr(5)
            | ehci_qh_set_endpt(2)
            | ehci_qh_set_eps(EHCI_QH_SPEED_HIGH)
            | ehci_qh_set_mpl(512)
            | ehci_qh_set_nrl(4);
        assert_eq!(ehci_qh_get_addr(endp), 5);
        assert_eq!(ehci_qh_get_endpt(endp), 2);
        assert_eq!(ehci_qh_get_eps(endp), EHCI_QH_SPEED_HIGH);
        assert_eq!(ehci_qh_get_mpl(endp), 512);
        assert_eq!(ehci_qh_get_nrl(endp), 4);
        assert_eq!(ehci_qh_get_dtc(endp | EHCI_QH_DTC), 1);
        let hub = ehci_qh_set_mult(1)
            | ehci_qh_set_smask(0x08)
            | ehci_qh_set_huba(3)
            | ehci_qh_set_port(4)
            | ehci_qh_set_cmask(0xe0);
        assert_eq!(ehci_qh_get_mult(hub), 1);
        assert_eq!(ehci_qh_get_smask(hub), 0x08);
        assert_eq!(ehci_qh_get_cmask(hub), 0xe0);
        assert_eq!(ehci_qh_get_huba(hub), 3);
        assert_eq!(ehci_qh_get_port(hub), 4);
    }

    #[test]
    fn isochronous_fields() {
        let ctl = EHCI_ITD_ACTIVE
            | ehci_itd_set_len(0x3ff)
            | ehci_itd_set_pg(6)
            | ehci_itd_set_offs(0x123)
            | ehci_itd_set_ioc(1);
        assert_eq!(ehci_itd_get_status(ctl), 0x8);
        assert_eq!(ehci_itd_get_len(ctl), 0x3ff);
        assert_eq!(ehci_itd_get_pg(ctl), 6);
        assert_eq!(ehci_itd_get_offs(ctl), 0x123);
        assert_eq!(ehci_itd_get_ioc(ctl), 1);
        assert_eq!(ehci_itd_set_status(0xf), 0xf000_0000);
        let b0 = ehci_itd_set_endpt(3) | ehci_itd_set_daddr(9);
        assert_eq!(ehci_itd_get_endpt(b0), 3);
        assert_eq!(ehci_itd_get_daddr(b0), 9);
        let b1 = ehci_itd_set_dir(1) | ehci_itd_set_maxpkt(1024);
        assert_eq!(ehci_itd_get_dir(b1), 1);
        assert_eq!(ehci_itd_get_maxpkt(b1), 1024);
        assert_eq!(ehci_itd_get_multi(ehci_itd_set_multi(2)), 2);

        let endp = ehci_sitd_set_endpt(1)
            | ehci_sitd_set_addr(7)
            | ehci_sitd_set_port(2)
            | ehci_sitd_set_huba(4)
            | ehci_sitd_set_dir(1);
        assert_eq!(ehci_sitd_get_endpt(endp), 1);
        assert_eq!(ehci_sitd_get_addr(endp), 7);
        assert_eq!(ehci_sitd_get_port(endp), 2);
        assert_eq!(ehci_sitd_get_huba(endp), 4);
        assert_eq!(ehci_sitd_get_dir(endp), 1);
        let sched = ehci_sitd_set_smask(0x01) | ehci_sitd_set_cmask(0x3c);
        assert_eq!(ehci_sitd_get_smask(sched), 0x01);
        assert_eq!(ehci_sitd_get_cmask(sched), 0x3c);
        let trans = EHCI_SITD_ACTIVE | ehci_sitd_set_len(188) | ehci_sitd_set_pg(1);
        assert_eq!(ehci_sitd_get_len(trans), 188);
        assert_eq!(ehci_sitd_get_pg(trans), 1);
        let page = ehci_sitd_set_tcount(2) | ehci_sitd_set_tp(EHCI_SITD_TP_BEGIN);
        assert_eq!(ehci_sitd_get_tcount(page), 2);
        assert_eq!(ehci_sitd_get_tp(page), EHCI_SITD_TP_BEGIN);
    }

    #[test]
    fn links_and_pages() {
        assert_eq!(ehci_link_type(0x1234_5642), EHCI_LINK_QH);
        assert_eq!(ehci_link_type(0x1234_5644), EHCI_LINK_SITD);
        assert_eq!(ehci_link_addr(0x1234_5663), 0x1234_5660);
        assert_eq!(ehci_page(0x1234_5678), 0x1234_5000);
        assert_eq!(ehci_page_offset(0x1234_5678), 0x678);
    }

    #[test]
    fn little_endian_access() {
        let qtd = EhciQtd {
            qtd_next: Cell::new(0),
            qtd_altnext: Cell::new(0),
            qtd_status: Cell::new(0),
            qtd_buffer: Default::default(),
            qtd_buffer_hi: Default::default(),
        };
        ehci_set(&qtd.qtd_status, 0x8000_0c80);
        assert_eq!(qtd.qtd_status.get(), 0x8000_0c80u32.to_le());
        assert_eq!(ehci_get(&qtd.qtd_status), 0x8000_0c80);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/usb/ehcireg.h");
        let ours = crate::reftest::assert_defines!(defs;
            PCI_CBMEM, PCI_INTERFACE_EHCI, PCI_USBREV, PCI_USBREV_MASK, PCI_USBREV_PRE_1_0,
            PCI_USBREV_1_0, PCI_USBREV_1_1, PCI_USBREV_2_0, PCI_EHCI_FLADJ, PCI_EHCI_PORTWAKECAP,
            EHCI_EC_LEGSUP, EHCI_LEGSUP_LEGSUP, EHCI_LEGSUP_OSOWNED, EHCI_LEGSUP_BIOSOWNED,
            PCI_LEGSUP_USBLEGCTLSTS,
            EHCI_CAPLENGTH, EHCI_HCIVERSION, EHCI_HCSPARAMS, EHCI_HCCPARAMS, EHCI_HCSP_PORTROUTE,
            EHCI_USBCMD, EHCI_CMD_ITC_M, EHCI_CMD_ITC_1, EHCI_CMD_ITC_2, EHCI_CMD_ITC_4,
            EHCI_CMD_ITC_8, EHCI_CMD_ITC_16, EHCI_CMD_ITC_32, EHCI_CMD_ITC_64, EHCI_CMD_ASPME,
            EHCI_CMD_ASPMC, EHCI_CMD_LHCR, EHCI_CMD_IAAD, EHCI_CMD_ASE, EHCI_CMD_PSE,
            EHCI_CMD_FLS_M, EHCI_CMD_HCRESET, EHCI_CMD_RS,
            EHCI_USBSTS, EHCI_STS_ASS, EHCI_STS_PSS, EHCI_STS_REC, EHCI_STS_HCH, EHCI_STS_IAA,
            EHCI_STS_HSE, EHCI_STS_FLR, EHCI_STS_PCD, EHCI_STS_ERRINT, EHCI_STS_INT,
            EHCI_NORMAL_INTRS,
            EHCI_USBINTR, EHCI_INTR_IAAE, EHCI_INTR_HSEE, EHCI_INTR_FLRE, EHCI_INTR_PCIE,
            EHCI_INTR_UEIE, EHCI_INTR_UIE, EHCI_FRINDEX, EHCI_CTRLDSSEGMENT,
            EHCI_PERIODICLISTBASE, EHCI_ASYNCLISTADDR, EHCI_CONFIGFLAG, EHCI_CONF_CF,
            EHCI_PS_WKOC_E, EHCI_PS_WKDSCNNT_E, EHCI_PS_WKCNNT_E, EHCI_PS_PTC, EHCI_PS_PIC,
            EHCI_PS_PO, EHCI_PS_PP, EHCI_PS_LS, EHCI_PS_PR, EHCI_PS_SUSP, EHCI_PS_FPR,
            EHCI_PS_OCC, EHCI_PS_OCA, EHCI_PS_PEC, EHCI_PS_PE, EHCI_PS_CSC, EHCI_PS_CS,
            EHCI_PS_CLEAR, EHCI_PORT_RESET_COMPLETE,
            EHCI_USBMODE, EHCI_USBMODE_CM_M, EHCI_USBMODE_CM_IDLE, EHCI_USBMODE_CM_DEVICE,
            EHCI_USBMODE_CM_HOST, EHCI_FLALIGN_ALIGN, EHCI_PAGE_SIZE,
            EHCI_LINK_TERMINATE, EHCI_LINK_ITD, EHCI_LINK_QH, EHCI_LINK_SITD, EHCI_LINK_FSTN,
            EHCI_BUFPTR_MASK, EHCI_ITD_NTRANS, EHCI_ITD_NBUFFERS,
            EHCI_ITD_ACTIVE, EHCI_ITD_BUF_ERR, EHCI_ITD_BABBLE, EHCI_ITD_ERROR, EHCI_ITD_IOC,
            EHCI_ITD_ALIGN,
            EHCI_SITD_IOC, EHCI_SITD_ACTIVE, EHCI_SITD_ERR, EHCI_SITD_BUFERR, EHCI_SITD_BABBLE,
            EHCI_SITD_XACTERR, EHCI_SITD_MISSEDMICRO, EHCI_SITD_SPLITXSTATE, EHCI_SITD_TP_ALL,
            EHCI_SITD_TP_BEGIN, EHCI_SITD_TP_MIDDLE, EHCI_SITD_TP_END, EHCI_SITD_ALIGN,
            EHCI_QTD_NBUFFERS, EHCI_QTD_ACTIVE, EHCI_QTD_HALTED, EHCI_QTD_BUFERR,
            EHCI_QTD_BABBLE, EHCI_QTD_XACTERR, EHCI_QTD_MISSEDMICRO, EHCI_QTD_SPLITXSTATE,
            EHCI_QTD_PINGSTATE, EHCI_QTD_STATERRS, EHCI_QTD_PID_OUT, EHCI_QTD_PID_IN,
            EHCI_QTD_PID_SETUP, EHCI_QTD_IOC, EHCI_QTD_TOGGLE_MASK, EHCI_QTD_ALIGN,
            EHCI_QH_ADDRMASK, EHCI_QH_INACT, EHCI_QH_SPEED_FULL, EHCI_QH_SPEED_LOW,
            EHCI_QH_SPEED_HIGH, EHCI_QH_DTC, EHCI_QH_HRECL, EHCI_QH_MPLMASK, EHCI_QH_CTL,
            EHCI_QH_ALIGN, EHCI_FSTN_ALIGN,
        );
        crate::reftest::assert_complete(&defs, "EHCI_", &ours);
        crate::reftest::assert_complete(&defs, "PCI_", &ours);
    }
}
/* </TESTS> */
