/* <LICENSES> */
/*
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Lennart Augustsson (lennart@augustsson.net) at
 * Carlstedt Research & Technology.
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
//! The OHCI registers and the structures the controller shares with the driver in memory:
//! `<dev/usb/ohcireg.h>`.
//!
//! Upstream: sys/dev/usb/ohcireg.h @ 3ce1f3f79392
//!
//! The operational registers sit at the start of the memory BAR, all 32 bits wide. The host
//! controller communications area (`struct ohci_hcca`: the 32 interrupt list heads, the
//! frame number and the done queue head), the endpoint descriptors (`struct ohci_ed`), the
//! general transfer descriptors (`struct ohci_td`) and the isochronous ones (`struct
//! ohci_itd`) live in DMA memory, in the controller's little-endian layout, and every pointer
//! between them is a 32-bit bus address (`ohci_physaddr_t`).
//!
//! ## Deviations
//! - Register offsets are `usize` (`bus_size_t`); register bits, addresses and descriptor
//!   fields `u32`; `PCI_CBMEM` is `i32` (`pci_mapreg_map`'s `int reg`).
//! - Function-like macros (`OHCI_RH_PORT_STATUS(n)`, `OHCI_TD_GET_CC(x)`, ...) are `const
//!   fn`s with the macro's name in lower case (`docs/C_TO_RUST.md`). `OHCI_ED_SET_FA(s)` and
//!   `OHCI_ITD_SET_SF(x)` mask nothing more than the C does.
//! - The descriptors are `#[repr(C)]` structures of `Cell<u32>` (and `Cell<u16>` for an
//!   isochronous descriptor's offsets) with the C's sizes pinned by compile-time checks. The
//!   driver keeps its software state next to them in the same DMA chunk (`ohcivar.rs`), so it
//!   reaches them as `&'static` and every access to a member is one volatile load or store
//!   through [`ohci_get`], [`ohci_set`], [`ohci_get16`] and [`ohci_set16`], which also do the
//!   `letoh32`/`htole32` (`letoh16`/`htole16`) conversions (`docs/C_TO_RUST.md`, a structure a
//!   chip and the driver share in DMA memory). The C relies on coherent memory and plain
//!   accesses; here every access is volatile.
//! - `#define itd_pswn itd_offset` (the same words read back as packet status words) is the
//!   one member [`OhciItd::itd_offset`].

use core::cell::Cell;
use core::ptr;

use crate::machine::bus::BusSize;

/*** PCI config registers ***/

/// `PCI_CBMEM`: configuration base memory.
pub const PCI_CBMEM: i32 = 0x10;

/// `PCI_INTERFACE_OHCI`.
pub const PCI_INTERFACE_OHCI: u32 = 0x10;

/*** OHCI registers */

/// `OHCI_REVISION`: OHCI revision #.
pub const OHCI_REVISION: BusSize = 0x00;

/// `OHCI_REV_LO(rev)`.
pub const fn ohci_rev_lo(rev: u32) -> u32 {
    rev & 0xf
}

/// `OHCI_REV_HI(rev)`.
pub const fn ohci_rev_hi(rev: u32) -> u32 {
    (rev >> 4) & 0xf
}

/// `OHCI_REV_LEGACY(rev)`.
pub const fn ohci_rev_legacy(rev: u32) -> u32 {
    rev & 0x100
}

/// `OHCI_CONTROL`.
pub const OHCI_CONTROL: BusSize = 0x04;
/// `OHCI_CBSR_MASK`: Control/Bulk Service Ratio.
pub const OHCI_CBSR_MASK: u32 = 0x00000003;
/// `OHCI_RATIO_1_1`.
pub const OHCI_RATIO_1_1: u32 = 0x00000000;
/// `OHCI_RATIO_1_2`.
pub const OHCI_RATIO_1_2: u32 = 0x00000001;
/// `OHCI_RATIO_1_3`.
pub const OHCI_RATIO_1_3: u32 = 0x00000002;
/// `OHCI_RATIO_1_4`.
pub const OHCI_RATIO_1_4: u32 = 0x00000003;
/// `OHCI_PLE`: Periodic List Enable.
pub const OHCI_PLE: u32 = 0x00000004;
/// `OHCI_IE`: Isochronous Enable.
pub const OHCI_IE: u32 = 0x00000008;
/// `OHCI_CLE`: Control List Enable.
pub const OHCI_CLE: u32 = 0x00000010;
/// `OHCI_BLE`: Bulk List Enable.
pub const OHCI_BLE: u32 = 0x00000020;
/// `OHCI_HCFS_MASK`: HostControllerFunctionalState.
pub const OHCI_HCFS_MASK: u32 = 0x000000c0;
/// `OHCI_HCFS_RESET`.
pub const OHCI_HCFS_RESET: u32 = 0x00000000;
/// `OHCI_HCFS_RESUME`.
pub const OHCI_HCFS_RESUME: u32 = 0x00000040;
/// `OHCI_HCFS_OPERATIONAL`.
pub const OHCI_HCFS_OPERATIONAL: u32 = 0x00000080;
/// `OHCI_HCFS_SUSPEND`.
pub const OHCI_HCFS_SUSPEND: u32 = 0x000000c0;
/// `OHCI_IR`: Interrupt Routing.
pub const OHCI_IR: u32 = 0x00000100;
/// `OHCI_RWC`: Remote Wakeup Connected.
pub const OHCI_RWC: u32 = 0x00000200;
/// `OHCI_RWE`: Remote Wakeup Enabled.
pub const OHCI_RWE: u32 = 0x00000400;
/// `OHCI_COMMAND_STATUS`.
pub const OHCI_COMMAND_STATUS: BusSize = 0x08;
/// `OHCI_HCR`: Host Controller Reset.
pub const OHCI_HCR: u32 = 0x00000001;
/// `OHCI_CLF`: Control List Filled.
pub const OHCI_CLF: u32 = 0x00000002;
/// `OHCI_BLF`: Bulk List Filled.
pub const OHCI_BLF: u32 = 0x00000004;
/// `OHCI_OCR`: Ownership Change Request.
pub const OHCI_OCR: u32 = 0x00000008;
/// `OHCI_SOC_MASK`: Scheduling Overrun Count.
pub const OHCI_SOC_MASK: u32 = 0x00030000;
/// `OHCI_INTERRUPT_STATUS`.
pub const OHCI_INTERRUPT_STATUS: BusSize = 0x0c;
/// `OHCI_SO`: Scheduling Overrun.
pub const OHCI_SO: u32 = 0x00000001;
/// `OHCI_WDH`: Writeback Done Head.
pub const OHCI_WDH: u32 = 0x00000002;
/// `OHCI_SF`: Start of Frame.
pub const OHCI_SF: u32 = 0x00000004;
/// `OHCI_RD`: Resume Detected.
pub const OHCI_RD: u32 = 0x00000008;
/// `OHCI_UE`: Unrecoverable Error.
pub const OHCI_UE: u32 = 0x00000010;
/// `OHCI_FNO`: Frame Number Overflow.
pub const OHCI_FNO: u32 = 0x00000020;
/// `OHCI_RHSC`: Root Hub Status Change.
pub const OHCI_RHSC: u32 = 0x00000040;
/// `OHCI_OC`: Ownership Change.
pub const OHCI_OC: u32 = 0x40000000;
/// `OHCI_MIE`: Master Interrupt Enable.
pub const OHCI_MIE: u32 = 0x80000000;
/// `OHCI_INTERRUPT_ENABLE`.
pub const OHCI_INTERRUPT_ENABLE: BusSize = 0x10;
/// `OHCI_INTERRUPT_DISABLE`.
pub const OHCI_INTERRUPT_DISABLE: BusSize = 0x14;
/// `OHCI_HCCA`.
pub const OHCI_HCCA: BusSize = 0x18;
/// `OHCI_PERIOD_CURRENT_ED`.
pub const OHCI_PERIOD_CURRENT_ED: BusSize = 0x1c;
/// `OHCI_CONTROL_HEAD_ED`.
pub const OHCI_CONTROL_HEAD_ED: BusSize = 0x20;
/// `OHCI_CONTROL_CURRENT_ED`.
pub const OHCI_CONTROL_CURRENT_ED: BusSize = 0x24;
/// `OHCI_BULK_HEAD_ED`.
pub const OHCI_BULK_HEAD_ED: BusSize = 0x28;
/// `OHCI_BULK_CURRENT_ED`.
pub const OHCI_BULK_CURRENT_ED: BusSize = 0x2c;
/// `OHCI_DONE_HEAD`.
pub const OHCI_DONE_HEAD: BusSize = 0x30;
/// `OHCI_FM_INTERVAL`.
pub const OHCI_FM_INTERVAL: BusSize = 0x34;

/// `OHCI_GET_IVAL(s)`.
pub const fn ohci_get_ival(s: u32) -> u32 {
    s & 0x3fff
}

/// `OHCI_GET_FSMPS(s)`.
pub const fn ohci_get_fsmps(s: u32) -> u32 {
    (s >> 16) & 0x7fff
}

/// `OHCI_FIT`.
pub const OHCI_FIT: u32 = 0x80000000;
/// `OHCI_FM_REMAINING`.
pub const OHCI_FM_REMAINING: BusSize = 0x38;
/// `OHCI_FM_NUMBER`.
pub const OHCI_FM_NUMBER: BusSize = 0x3c;
/// `OHCI_PERIODIC_START`.
pub const OHCI_PERIODIC_START: BusSize = 0x40;
/// `OHCI_LS_THRESHOLD`.
pub const OHCI_LS_THRESHOLD: BusSize = 0x44;
/// `OHCI_RH_DESCRIPTOR_A`.
pub const OHCI_RH_DESCRIPTOR_A: BusSize = 0x48;

/// `OHCI_GET_NDP(s)`.
pub const fn ohci_get_ndp(s: u32) -> u32 {
    s & 0xff
}

/// `OHCI_PSM`: Power Switching Mode.
pub const OHCI_PSM: u32 = 0x0100;
/// `OHCI_NPS`: No Power Switching.
pub const OHCI_NPS: u32 = 0x0200;
/// `OHCI_DT`: Device Type.
pub const OHCI_DT: u32 = 0x0400;
/// `OHCI_OCPM`: Overcurrent Protection Mode.
pub const OHCI_OCPM: u32 = 0x0800;
/// `OHCI_NOCP`: No Overcurrent Protection.
pub const OHCI_NOCP: u32 = 0x1000;

/// `OHCI_GET_POTPGT(s)`.
pub const fn ohci_get_potpgt(s: u32) -> u32 {
    s >> 24
}

/// `OHCI_RH_DESCRIPTOR_B`.
pub const OHCI_RH_DESCRIPTOR_B: BusSize = 0x4c;
/// `OHCI_RH_STATUS`.
pub const OHCI_RH_STATUS: BusSize = 0x50;
/// `OHCI_LPS`: Local Power Status.
pub const OHCI_LPS: u32 = 0x00000001;
/// `OHCI_OCI`: OverCurrent Indicator.
pub const OHCI_OCI: u32 = 0x00000002;
/// `OHCI_DRWE`: Device Remote Wakeup Enable.
pub const OHCI_DRWE: u32 = 0x00008000;
/// `OHCI_LPSC`: Local Power Status Change.
pub const OHCI_LPSC: u32 = 0x00010000;
/// `OHCI_CCIC`: OverCurrent Indicator Change.
pub const OHCI_CCIC: u32 = 0x00020000;
/// `OHCI_CRWE`: Clear Remote Wakeup Enable.
pub const OHCI_CRWE: u32 = 0x80000000;

/// `OHCI_RH_PORT_STATUS(n)`: 1 based indexing.
pub const fn ohci_rh_port_status(n: usize) -> BusSize {
    0x50 + n * 4
}

/// `OHCI_LES`.
pub const OHCI_LES: u32 = OHCI_PLE | OHCI_IE | OHCI_CLE | OHCI_BLE;
/// `OHCI_ALL_INTRS`.
pub const OHCI_ALL_INTRS: u32 =
    OHCI_SO | OHCI_WDH | OHCI_SF | OHCI_RD | OHCI_UE | OHCI_FNO | OHCI_RHSC | OHCI_OC;
/// `OHCI_NORMAL_INTRS`.
pub const OHCI_NORMAL_INTRS: u32 = OHCI_SO | OHCI_WDH | OHCI_RD | OHCI_UE | OHCI_RHSC;

/// `OHCI_FSMPS(i)`: the FS largest data packet for a frame interval of `i` bit times.
pub const fn ohci_fsmps(i: u32) -> u32 {
    (i.wrapping_sub(210).wrapping_mul(6) / 7) << 16
}

/// `OHCI_PERIODIC(i)`: 90% of the frame interval.
pub const fn ohci_periodic(i: u32) -> u32 {
    i * 9 / 10
}

/// `ohci_physaddr_t`.
pub type OhciPhysaddrT = u32;

/// `OHCI_NO_INTRS`.
pub const OHCI_NO_INTRS: usize = 32;

/// `struct ohci_hcca`: the host controller communications area.
#[repr(C)]
pub struct OhciHcca {
    /// `hcca_interrupt_table`.
    pub hcca_interrupt_table: [Cell<OhciPhysaddrT>; OHCI_NO_INTRS],
    /// `hcca_frame_number`.
    pub hcca_frame_number: Cell<u32>,
    /// `hcca_done_head`.
    pub hcca_done_head: Cell<OhciPhysaddrT>,
}

/// `OHCI_DONE_INTRS`.
pub const OHCI_DONE_INTRS: u32 = 1;
/// `OHCI_HCCA_SIZE`.
pub const OHCI_HCCA_SIZE: usize = 256;
/// `OHCI_HCCA_ALIGN`.
pub const OHCI_HCCA_ALIGN: usize = 256;

/// `OHCI_PAGE_SIZE`.
pub const OHCI_PAGE_SIZE: u32 = 0x1000;

/// `OHCI_PAGE(x)`.
pub const fn ohci_page(x: u32) -> u32 {
    x & !0xfff
}

/// `OHCI_PAGE_OFFSET(x)`.
pub const fn ohci_page_offset(x: u32) -> u32 {
    x & 0xfff
}

/// `struct ohci_ed`: an endpoint descriptor.
#[repr(C)]
pub struct OhciEd {
    /// `ed_flags`.
    pub ed_flags: Cell<u32>,
    /// `ed_tailp`.
    pub ed_tailp: Cell<OhciPhysaddrT>,
    /// `ed_headp`.
    pub ed_headp: Cell<OhciPhysaddrT>,
    /// `ed_nexted`.
    pub ed_nexted: Cell<OhciPhysaddrT>,
}

/// `OHCI_ED_GET_FA(s)`.
pub const fn ohci_ed_get_fa(s: u32) -> u32 {
    s & 0x7f
}

/// `OHCI_ED_ADDRMASK`.
pub const OHCI_ED_ADDRMASK: u32 = 0x0000007f;

/// `OHCI_ED_SET_FA(s)`.
pub const fn ohci_ed_set_fa(s: u32) -> u32 {
    s
}

/// `OHCI_ED_GET_EN(s)`.
pub const fn ohci_ed_get_en(s: u32) -> u32 {
    (s >> 7) & 0xf
}

/// `OHCI_ED_SET_EN(s)`.
pub const fn ohci_ed_set_en(s: u32) -> u32 {
    s << 7
}

/// `OHCI_ED_DIR_MASK`.
pub const OHCI_ED_DIR_MASK: u32 = 0x00001800;
/// `OHCI_ED_DIR_TD`.
pub const OHCI_ED_DIR_TD: u32 = 0x00000000;
/// `OHCI_ED_DIR_OUT`.
pub const OHCI_ED_DIR_OUT: u32 = 0x00000800;
/// `OHCI_ED_DIR_IN`.
pub const OHCI_ED_DIR_IN: u32 = 0x00001000;
/// `OHCI_ED_SPEED`.
pub const OHCI_ED_SPEED: u32 = 0x00002000;
/// `OHCI_ED_SKIP`.
pub const OHCI_ED_SKIP: u32 = 0x00004000;
/// `OHCI_ED_FORMAT_GEN`.
pub const OHCI_ED_FORMAT_GEN: u32 = 0x00000000;
/// `OHCI_ED_FORMAT_ISO`.
pub const OHCI_ED_FORMAT_ISO: u32 = 0x00008000;

/// `OHCI_ED_GET_MAXP(s)`.
pub const fn ohci_ed_get_maxp(s: u32) -> u32 {
    (s >> 16) & 0x07ff
}

/// `OHCI_ED_SET_MAXP(s)`.
pub const fn ohci_ed_set_maxp(s: u32) -> u32 {
    s << 16
}

/// `OHCI_ED_MAXPMASK`.
pub const OHCI_ED_MAXPMASK: u32 = 0x7ff << 16;
/// `OHCI_HALTED`.
pub const OHCI_HALTED: u32 = 0x00000001;
/// `OHCI_TOGGLECARRY`.
pub const OHCI_TOGGLECARRY: u32 = 0x00000002;
/// `OHCI_HEADMASK`.
pub const OHCI_HEADMASK: u32 = 0xfffffffc;

/* #define OHCI_ED_SIZE 16 */
/// `OHCI_ED_ALIGN`.
pub const OHCI_ED_ALIGN: usize = 16;

/// `struct ohci_td`: a general transfer descriptor.
#[repr(C)]
pub struct OhciTd {
    /// `td_flags`.
    pub td_flags: Cell<u32>,
    /// `td_cbp`: Current Buffer Pointer.
    pub td_cbp: Cell<OhciPhysaddrT>,
    /// `td_nexttd`: Next TD.
    pub td_nexttd: Cell<OhciPhysaddrT>,
    /// `td_be`: Buffer End.
    pub td_be: Cell<OhciPhysaddrT>,
}

/// `OHCI_TD_R`: Buffer Rounding.
pub const OHCI_TD_R: u32 = 0x00040000;
/// `OHCI_TD_DP_MASK`: Direction / PID.
pub const OHCI_TD_DP_MASK: u32 = 0x00180000;
/// `OHCI_TD_SETUP`.
pub const OHCI_TD_SETUP: u32 = 0x00000000;
/// `OHCI_TD_OUT`.
pub const OHCI_TD_OUT: u32 = 0x00080000;
/// `OHCI_TD_IN`.
pub const OHCI_TD_IN: u32 = 0x00100000;

/// `OHCI_TD_GET_DI(x)`: Delay Interrupt.
pub const fn ohci_td_get_di(x: u32) -> u32 {
    (x >> 21) & 7
}

/// `OHCI_TD_SET_DI(x)`.
pub const fn ohci_td_set_di(x: u32) -> u32 {
    x << 21
}

/// `OHCI_TD_NOINTR`.
pub const OHCI_TD_NOINTR: u32 = 0x00e00000;
/// `OHCI_TD_INTR_MASK`.
pub const OHCI_TD_INTR_MASK: u32 = 0x00e00000;
/// `OHCI_TD_TOGGLE_CARRY`.
pub const OHCI_TD_TOGGLE_CARRY: u32 = 0x00000000;
/// `OHCI_TD_TOGGLE_0`.
pub const OHCI_TD_TOGGLE_0: u32 = 0x02000000;
/// `OHCI_TD_TOGGLE_1`.
pub const OHCI_TD_TOGGLE_1: u32 = 0x03000000;
/// `OHCI_TD_TOGGLE_MASK`.
pub const OHCI_TD_TOGGLE_MASK: u32 = 0x03000000;

/// `OHCI_TD_GET_EC(x)`: Error Count.
pub const fn ohci_td_get_ec(x: u32) -> u32 {
    (x >> 26) & 3
}

/// `OHCI_TD_GET_CC(x)`: Condition Code.
pub const fn ohci_td_get_cc(x: u32) -> u32 {
    x >> 28
}

/// `OHCI_TD_NOCC`.
pub const OHCI_TD_NOCC: u32 = 0xf0000000;

/* #define OHCI_TD_SIZE 16 */
/// `OHCI_TD_ALIGN`.
pub const OHCI_TD_ALIGN: usize = 16;

/// `OHCI_ITD_NOFFSET`.
pub const OHCI_ITD_NOFFSET: usize = 8;

/// `struct ohci_itd`: an isochronous transfer descriptor.
#[repr(C)]
pub struct OhciItd {
    /// `itd_flags`.
    pub itd_flags: Cell<u32>,
    /// `itd_bp0`: Buffer Page 0.
    pub itd_bp0: Cell<OhciPhysaddrT>,
    /// `itd_nextitd`: Next ITD.
    pub itd_nextitd: Cell<OhciPhysaddrT>,
    /// `itd_be`: Buffer End.
    pub itd_be: Cell<OhciPhysaddrT>,
    /// `itd_offset`: Buffer offsets; also `itd_pswn`, the Packet Status Words.
    pub itd_offset: [Cell<u16>; OHCI_ITD_NOFFSET],
}

/// `OHCI_ITD_GET_SF(x)`.
pub const fn ohci_itd_get_sf(x: u32) -> u32 {
    x & 0x0000ffff
}

/// `OHCI_ITD_SET_SF(x)`.
pub const fn ohci_itd_set_sf(x: u32) -> u32 {
    x & 0xffff
}

/// `OHCI_ITD_GET_DI(x)`: Delay Interrupt.
pub const fn ohci_itd_get_di(x: u32) -> u32 {
    (x >> 21) & 7
}

/// `OHCI_ITD_SET_DI(x)`.
pub const fn ohci_itd_set_di(x: u32) -> u32 {
    x << 21
}

/// `OHCI_ITD_NOINTR`.
pub const OHCI_ITD_NOINTR: u32 = 0x00e00000;

/// `OHCI_ITD_GET_FC(x)`: Frame Count.
pub const fn ohci_itd_get_fc(x: u32) -> u32 {
    ((x >> 24) & 7) + 1
}

/// `OHCI_ITD_SET_FC(x)`.
pub const fn ohci_itd_set_fc(x: u32) -> u32 {
    x.wrapping_sub(1) << 24
}

/// `OHCI_ITD_GET_CC(x)`: Condition Code.
pub const fn ohci_itd_get_cc(x: u32) -> u32 {
    x >> 28
}

/// `OHCI_ITD_NOCC`.
pub const OHCI_ITD_NOCC: u32 = 0xf0000000;
/// `OHCI_ITD_PAGE_SELECT`.
pub const OHCI_ITD_PAGE_SELECT: u32 = 0x00001000;

/// `OHCI_ITD_MK_OFFS(len)`.
pub const fn ohci_itd_mk_offs(len: u32) -> u16 {
    (0xe000 | (len & 0x1fff)) as u16
}

/// `OHCI_ITD_PSW_LENGTH(x)`: Transfer length.
pub const fn ohci_itd_psw_length(x: u16) -> u16 {
    x & 0xfff
}

/// `OHCI_ITD_PSW_GET_CC(x)`: Condition Code.
pub const fn ohci_itd_psw_get_cc(x: u16) -> u16 {
    x >> 12
}

/* #define OHCI_ITD_SIZE 32 */
/// `OHCI_ITD_ALIGN`.
pub const OHCI_ITD_ALIGN: usize = 32;

/// `OHCI_CC_NO_ERROR`.
pub const OHCI_CC_NO_ERROR: u32 = 0;
/// `OHCI_CC_CRC`.
pub const OHCI_CC_CRC: u32 = 1;
/// `OHCI_CC_BIT_STUFFING`.
pub const OHCI_CC_BIT_STUFFING: u32 = 2;
/// `OHCI_CC_DATA_TOGGLE_MISMATCH`.
pub const OHCI_CC_DATA_TOGGLE_MISMATCH: u32 = 3;
/// `OHCI_CC_STALL`.
pub const OHCI_CC_STALL: u32 = 4;
/// `OHCI_CC_DEVICE_NOT_RESPONDING`.
pub const OHCI_CC_DEVICE_NOT_RESPONDING: u32 = 5;
/// `OHCI_CC_PID_CHECK_FAILURE`.
pub const OHCI_CC_PID_CHECK_FAILURE: u32 = 6;
/// `OHCI_CC_UNEXPECTED_PID`.
pub const OHCI_CC_UNEXPECTED_PID: u32 = 7;
/// `OHCI_CC_DATA_OVERRUN`.
pub const OHCI_CC_DATA_OVERRUN: u32 = 8;
/// `OHCI_CC_DATA_UNDERRUN`.
pub const OHCI_CC_DATA_UNDERRUN: u32 = 9;
/// `OHCI_CC_BUFFER_OVERRUN`.
pub const OHCI_CC_BUFFER_OVERRUN: u32 = 12;
/// `OHCI_CC_BUFFER_UNDERRUN`.
pub const OHCI_CC_BUFFER_UNDERRUN: u32 = 13;
/// `OHCI_CC_NOT_ACCESSED`.
pub const OHCI_CC_NOT_ACCESSED: u32 = 14;
/// `OHCI_CC_NOT_ACCESSED_MASK`.
pub const OHCI_CC_NOT_ACCESSED_MASK: u32 = 14;

/* Some delay needed when changing certain registers. */
/// `OHCI_ENABLE_POWER_DELAY`, in ms.
pub const OHCI_ENABLE_POWER_DELAY: u32 = 5;
/// `OHCI_READ_DESC_DELAY`, in ms.
pub const OHCI_READ_DESC_DELAY: u32 = 5;

/// `letoh32(member)`: one volatile load of a descriptor member in DMA memory, converted from
/// the controller's little-endian layout.
#[inline]
pub fn ohci_get(c: &Cell<u32>) -> u32 {
    // SAFETY: the cell's own four bytes, aligned (`#[repr(C)]` in an aligned descriptor); the
    // controller writes the memory, so the load is volatile; any bits are a `u32`.
    u32::from_le(unsafe { ptr::read_volatile(c.as_ptr()) })
}

/// `member = htole32(v)`: one volatile store of a descriptor member in DMA memory.
#[inline]
pub fn ohci_set(c: &Cell<u32>, v: u32) {
    // SAFETY: the cell's own four bytes, aligned; a `Cell` may be written through a shared
    // reference, and the controller reads the memory, so the store is volatile.
    unsafe { ptr::write_volatile(c.as_ptr(), v.to_le()) }
}

/// `letoh16(member)`: one volatile load of an isochronous offset or packet status word.
#[inline]
pub fn ohci_get16(c: &Cell<u16>) -> u16 {
    // SAFETY: as for `ohci_get`, for the cell's two bytes.
    u16::from_le(unsafe { ptr::read_volatile(c.as_ptr()) })
}

/// `member = htole16(v)`: one volatile store of an isochronous offset.
#[inline]
pub fn ohci_set16(c: &Cell<u16>, v: u16) {
    // SAFETY: as for `ohci_set`, for the cell's two bytes.
    unsafe { ptr::write_volatile(c.as_ptr(), v.to_le()) }
}

const _: () = {
    assert!(size_of::<OhciHcca>() == 136);
    assert!(size_of::<OhciHcca>() <= OHCI_HCCA_SIZE);
    assert!(size_of::<OhciEd>() == 16);
    assert!(size_of::<OhciTd>() == 16);
    assert!(size_of::<OhciItd>() == 32);
    assert!(core::mem::offset_of!(OhciHcca, hcca_done_head) == 0x84);
    assert!(core::mem::offset_of!(OhciItd, itd_offset) == 16);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_macros() {
        // QEMU's pci-ohci: revision 0x10, three ports, 1-ms power-on to power-good.
        assert_eq!(ohci_rev_hi(0x10), 1);
        assert_eq!(ohci_rev_lo(0x10), 0);
        assert_eq!(ohci_rev_legacy(0x110), 0x100);
        assert_eq!(ohci_rev_legacy(0x10), 0);
        assert_eq!(ohci_rh_port_status(1), 0x54);
        assert_eq!(ohci_rh_port_status(3), 0x5c);
        assert_eq!(ohci_get_ndp(0x0100_0203), 3);
        assert_eq!(ohci_get_potpgt(0x0100_0203), 1);
        // The usual 12000-bit frame interval (11999 = 0x2edf).
        let fm = 0x2edf;
        assert_eq!(ohci_get_ival(0xa782_2edf), fm);
        assert_eq!(ohci_get_fsmps(0xa782_2edf), 0x2782);
        assert_eq!(ohci_fsmps(fm), 0x2778 << 16);
        assert_eq!(ohci_periodic(fm), 0x2a2f);
        assert_eq!(OHCI_LES, 0x3c);
        assert_eq!(OHCI_ALL_INTRS, 0x4000_007f);
        assert_eq!(OHCI_NORMAL_INTRS, 0x5b);
    }

    #[test]
    fn descriptor_fields() {
        let ed = ohci_ed_set_fa(5)
            | ohci_ed_set_en(2)
            | OHCI_ED_SPEED
            | OHCI_ED_FORMAT_ISO
            | OHCI_ED_DIR_IN
            | ohci_ed_set_maxp(1023);
        assert_eq!(ohci_ed_get_fa(ed), 5);
        assert_eq!(ohci_ed_get_en(ed), 2);
        assert_eq!(ohci_ed_get_maxp(ed), 1023);
        assert_eq!(ed & OHCI_ED_DIR_MASK, OHCI_ED_DIR_IN);
        assert_eq!(ed & OHCI_ED_MAXPMASK, 1023 << 16);

        let td = OHCI_TD_IN | OHCI_TD_NOCC | ohci_td_set_di(1) | OHCI_TD_TOGGLE_1;
        assert_eq!(ohci_td_get_di(td), 1);
        assert_eq!(ohci_td_get_cc(td), 0xf);
        assert_eq!(ohci_td_get_ec(td), 0);
        assert_eq!(td & OHCI_TD_TOGGLE_MASK, OHCI_TD_TOGGLE_1);
        assert_eq!(td & OHCI_TD_DP_MASK, OHCI_TD_IN);
        assert_eq!(ohci_td_get_cc(0x4000_0000), OHCI_CC_STALL);

        let itd =
            OHCI_ITD_NOCC | ohci_itd_set_sf(0x1_2345) | ohci_itd_set_di(6) | ohci_itd_set_fc(8);
        assert_eq!(ohci_itd_get_sf(itd), 0x2345);
        assert_eq!(ohci_itd_get_di(itd), 6);
        assert_eq!(ohci_itd_get_fc(itd), 8);
        assert_eq!(ohci_itd_get_cc(itd), 0xf);
        assert_eq!(ohci_itd_get_fc(ohci_itd_set_fc(1)), 1);
        assert_eq!(ohci_itd_mk_offs(0x123), 0xe123);
        assert_eq!(ohci_itd_mk_offs(0x3000), 0xf000);
        assert_eq!(ohci_itd_psw_length(0x0040), 0x40);
        assert_eq!(ohci_itd_psw_get_cc(0xe000), OHCI_CC_NOT_ACCESSED as u16);
    }

    #[test]
    fn pages() {
        assert_eq!(ohci_page(0x1234_5678), 0x1234_5000);
        assert_eq!(ohci_page_offset(0x1234_5678), 0x678);
    }

    #[test]
    fn dma_accessors_are_little_endian() {
        let c = Cell::new(0u32);
        ohci_set(&c, 0x1122_3344);
        assert_eq!(c.get(), 0x1122_3344u32.to_le());
        assert_eq!(ohci_get(&c), 0x1122_3344);
        let h = Cell::new(0u16);
        ohci_set16(&h, 0xe123);
        assert_eq!(h.get(), 0xe123u16.to_le());
        assert_eq!(ohci_get16(&h), 0xe123);
    }
}
/* </TESTS> */
