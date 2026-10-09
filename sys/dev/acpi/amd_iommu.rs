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
 * Copyright (c) 2019 Jordan Hargrave <jordan_hargrave@hotmail.com>
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
/* </LICENSES> */

/* <CODE> */
//! `dev/acpi/amd_iommu.h`: the AMD-Vi (AMD I/O Virtualization) registers, the device table
//! entry, the command and event formats, and the page table bits `acpidmar.c`'s IVRS path
//! programs.
//!
//! Upstream: sys/dev/acpi/amd_iommu.h @ 3ce1f3f79392
//!
//! The header has no `$OpenBSD$` line. The prototypes it declares (`ivhd_flush_devtab`,
//! `ivhd_invalidate_iommu_all`, `ivhd_invalidate_interrupt_table`, `ivhd_issue_command`,
//! `ivhd_invalidate_domain`, `_dumppte`) are `acpidmar.rs`'s functions.
//!
//! ## Deviations
//! - `struct ivhd_dte` (eight `uint32_t`s, `__packed`) is four `AtomicU64`s: the table is
//!   shared with the IOMMU and written by whichever CPU attaches a device, and the C reads and
//!   writes `dw0`/`dw1` and `dw4`/`dw5` as one 64-bit word (`_get64`/`_put64`). The 32-bit
//!   words are [`IvhdDte::dw`] and [`IvhdDte::set_dw`] (little endian, as the hardware's);
//!   the entry is 8-byte aligned where the C's is byte aligned (the table is a page-aligned
//!   allocation, so the layout is the same).
//! - `iommu_rmw32` (from `acpidmar.h`) returns the new word instead of changing it in place.

use core::sync::atomic::{AtomicU64, Ordering};

use super::acpidmar::iommu_rmw32;

/// `DEV_TAB_BASE_REG`.
pub const DEV_TAB_BASE_REG: usize = 0x0000;
/// `CMD_BASE_REG`.
pub const CMD_BASE_REG: usize = 0x0008;
/// `EVT_BASE_REG`.
pub const EVT_BASE_REG: usize = 0x0010;

/// `EXCL_BASE_REG`.
pub const EXCL_BASE_REG: usize = 0x0020;
/// `EXCL_LIMIT_REG`.
pub const EXCL_LIMIT_REG: usize = 0x0028;

/// `EXTFEAT_REG`: Extended Feature Register.
pub const EXTFEAT_REG: usize = 0x0030;
/// `EFR_PREFSUP`.
pub const EFR_PREFSUP: u64 = 1 << 0;
/// `EFR_PPRSUP`.
pub const EFR_PPRSUP: u64 = 1 << 1;
/// `EFR_NXSUP`.
pub const EFR_NXSUP: u64 = 1 << 3;
/// `EFR_GTSUP`.
pub const EFR_GTSUP: u64 = 1 << 4;
/// `EFR_IASUP`.
pub const EFR_IASUP: u64 = 1 << 6;
/// `EFR_GASUP`.
pub const EFR_GASUP: u64 = 1 << 7;
/// `EFR_HESUP`.
pub const EFR_HESUP: u64 = 1 << 8;
/// `EFR_PCSUP`.
pub const EFR_PCSUP: u64 = 1 << 9;
/// `EFR_HATS_SHIFT`.
pub const EFR_HATS_SHIFT: u64 = 10;
/// `EFR_HATS_MASK`.
pub const EFR_HATS_MASK: u64 = 0x3;
/// `EFR_GATS_SHIFT`.
pub const EFR_GATS_SHIFT: u64 = 12;
/// `EFR_GATS_MASK`.
pub const EFR_GATS_MASK: u64 = 0x3;
/// `EFR_GLXSUP_SHIFT`.
pub const EFR_GLXSUP_SHIFT: u64 = 14;
/// `EFR_GLXSUP_MASK`.
pub const EFR_GLXSUP_MASK: u64 = 0x3;
/// `EFR_SMIFSUP_SHIFT`.
pub const EFR_SMIFSUP_SHIFT: u64 = 16;
/// `EFR_SMIFSUP_MASK`.
pub const EFR_SMIFSUP_MASK: u64 = 0x3;
/// `EFR_SMIFRC_SHIFT`.
pub const EFR_SMIFRC_SHIFT: u64 = 18;
/// `EFR_SMIFRC_MASK`.
pub const EFR_SMIFRC_MASK: u64 = 0x7;
/// `EFR_GAMSUP_SHIFT`.
pub const EFR_GAMSUP_SHIFT: u64 = 21;
/// `EFR_GAMSUP_MASK`.
pub const EFR_GAMSUP_MASK: u64 = 0x7;

/// `CMD_HEAD_REG`.
pub const CMD_HEAD_REG: usize = 0x2000;
/// `CMD_TAIL_REG`.
pub const CMD_TAIL_REG: usize = 0x2008;
/// `EVT_HEAD_REG`.
pub const EVT_HEAD_REG: usize = 0x2010;
/// `EVT_TAIL_REG`.
pub const EVT_TAIL_REG: usize = 0x2018;

/// `IOMMUSTS_REG`.
pub const IOMMUSTS_REG: usize = 0x2020;

/// `DEV_TAB_MASK`.
pub const DEV_TAB_MASK: u64 = 0x000F_FFFF_FFFF_F000;
/// `DEV_TAB_LEN`.
pub const DEV_TAB_LEN: u64 = 0x1FF;

/// `IOMMUCTL_REG`: IOMMU Control.
pub const IOMMUCTL_REG: usize = 0x0018;
/// `CTL_IOMMUEN`.
pub const CTL_IOMMUEN: u64 = 1 << 0;
/// `CTL_HTTUNEN`.
pub const CTL_HTTUNEN: u64 = 1 << 1;
/// `CTL_EVENTLOGEN`.
pub const CTL_EVENTLOGEN: u64 = 1 << 2;
/// `CTL_EVENTINTEN`.
pub const CTL_EVENTINTEN: u64 = 1 << 3;
/// `CTL_COMWAITINTEN`.
pub const CTL_COMWAITINTEN: u64 = 1 << 4;
/// `CTL_INVTIMEOUT_SHIFT`.
pub const CTL_INVTIMEOUT_SHIFT: u64 = 5;
/// `CTL_INVTIMEOUT_MASK`.
pub const CTL_INVTIMEOUT_MASK: u64 = 0x7;
/// `CTL_INVTIMEOUT_NONE`.
pub const CTL_INVTIMEOUT_NONE: u64 = 0;
/// `CTL_INVTIMEOUT_1MS`.
pub const CTL_INVTIMEOUT_1MS: u64 = 1;
/// `CTL_INVTIMEOUT_10MS`.
pub const CTL_INVTIMEOUT_10MS: u64 = 2;
/// `CTL_INVTIMEOUT_100MS`.
pub const CTL_INVTIMEOUT_100MS: u64 = 3;
/// `CTL_INVTIMEOUT_1S`.
pub const CTL_INVTIMEOUT_1S: u64 = 4;
/// `CTL_INVTIMEOUT_10S`.
pub const CTL_INVTIMEOUT_10S: u64 = 5;
/// `CTL_INVTIMEOUT_100S`.
pub const CTL_INVTIMEOUT_100S: u64 = 6;
/// `CTL_PASSPW`.
pub const CTL_PASSPW: u64 = 1 << 8;
/// `CTL_RESPASSPW`.
pub const CTL_RESPASSPW: u64 = 1 << 9;
/// `CTL_COHERENT`.
pub const CTL_COHERENT: u64 = 1 << 10;
/// `CTL_ISOC`.
pub const CTL_ISOC: u64 = 1 << 11;
/// `CTL_CMDBUFEN`.
pub const CTL_CMDBUFEN: u64 = 1 << 12;
/// `CTL_PPRLOGEN`.
pub const CTL_PPRLOGEN: u64 = 1 << 13;
/// `CTL_PPRINTEN`.
pub const CTL_PPRINTEN: u64 = 1 << 14;
/// `CTL_PPREN`.
pub const CTL_PPREN: u64 = 1 << 15;
/// `CTL_GTEN`.
pub const CTL_GTEN: u64 = 1 << 16;
/// `CTL_GAEN`.
pub const CTL_GAEN: u64 = 1 << 17;
/// `CTL_CRW_SHIFT`.
pub const CTL_CRW_SHIFT: u64 = 18;
/// `CTL_CRW_MASK`.
pub const CTL_CRW_MASK: u64 = 0xF;
/// `CTL_SMIFEN`.
pub const CTL_SMIFEN: u64 = 1 << 22;
/// `CTL_SLFWBDIS`.
pub const CTL_SLFWBDIS: u64 = 1 << 23;
/// `CTL_SMIFLOGEN`.
pub const CTL_SMIFLOGEN: u64 = 1 << 24;
/// `CTL_GAMEN_SHIFT`.
pub const CTL_GAMEN_SHIFT: u64 = 25;
/// `CTL_GAMEN_MASK`.
pub const CTL_GAMEN_MASK: u64 = 0x7;
/// `CTL_GALOGEN`.
pub const CTL_GALOGEN: u64 = 1 << 28;
/// `CTL_GAINTEN`.
pub const CTL_GAINTEN: u64 = 1 << 29;
/// `CTL_DUALPPRLOGEN_SHIFT`.
pub const CTL_DUALPPRLOGEN_SHIFT: u64 = 30;
/// `CTL_DUALPPRLOGEN_MASK`.
pub const CTL_DUALPPRLOGEN_MASK: u64 = 0x3;
/// `CTL_DUALEVTLOGEN_SHIFT`.
pub const CTL_DUALEVTLOGEN_SHIFT: u64 = 32;
/// `CTL_DUALEVTLOGEN_MASK`.
pub const CTL_DUALEVTLOGEN_MASK: u64 = 0x3;
/// `CTL_DEVTBLSEGEN_SHIFT`.
pub const CTL_DEVTBLSEGEN_SHIFT: u64 = 34;
/// `CTL_DEVTBLSEGEN_MASK`.
pub const CTL_DEVTBLSEGEN_MASK: u64 = 0x7;
/// `CTL_PRIVABRTEN_SHIFT`.
pub const CTL_PRIVABRTEN_SHIFT: u64 = 37;
/// `CTL_PRIVABRTEN_MASK`.
pub const CTL_PRIVABRTEN_MASK: u64 = 0x3;
/// `CTL_PPRAUTORSPEN`.
pub const CTL_PPRAUTORSPEN: u64 = 1 << 39;
/// `CTL_MARCEN`.
pub const CTL_MARCEN: u64 = 1 << 40;
/// `CTL_BLKSTOPMRKEN`.
pub const CTL_BLKSTOPMRKEN: u64 = 1 << 41;
/// `CTL_PPRAUTOSPAON`.
pub const CTL_PPRAUTOSPAON: u64 = 1 << 42;
/// `CTL_DOMAINIDPNE`.
pub const CTL_DOMAINIDPNE: u64 = 1 << 43;

/// `CMD_BASE_MASK`.
pub const CMD_BASE_MASK: u64 = 0x000F_FFFF_FFFF_F000;
/// `CMD_TBL_SIZE`.
pub const CMD_TBL_SIZE: u32 = 4096;
/// `CMD_TBL_LEN_4K`.
pub const CMD_TBL_LEN_4K: u64 = 8 << 56;
/// `CMD_TBL_LEN_8K`.
pub const CMD_TBL_LEN_8K: u64 = 9 << 56;

/// `EVT_BASE_MASK`.
pub const EVT_BASE_MASK: u64 = 0x000F_FFFF_FFFF_F000;
/// `EVT_TBL_SIZE`.
pub const EVT_TBL_SIZE: u32 = 4096;
/// `EVT_TBL_LEN_4K`.
pub const EVT_TBL_LEN_4K: u64 = 8 << 56;
/// `EVT_TBL_LEN_8K`.
pub const EVT_TBL_LEN_8K: u64 = 9 << 56;

/// `HWDTE_SIZE`: one device table entry per source ID.
pub const HWDTE_SIZE: usize = 65536 * size_of::<IvhdDte>();

/// `DTE_V` (dw0).
pub const DTE_V: u32 = 1 << 0;
/// `DTE_TV` (dw0).
pub const DTE_TV: u32 = 1 << 1;
/// `DTE_LEVEL_SHIFT` (dw0).
pub const DTE_LEVEL_SHIFT: u32 = 9;
/// `DTE_LEVEL_MASK` (dw0).
pub const DTE_LEVEL_MASK: u32 = 0x7;
/// `DTE_HPTRP_MASK` (dw0,1).
pub const DTE_HPTRP_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// `DTE_PPR` (dw1).
pub const DTE_PPR: u32 = 1 << 20;
/// `DTE_GPRP` (dw1).
pub const DTE_GPRP: u32 = 1 << 21;
/// `DTE_GIOV` (dw1).
pub const DTE_GIOV: u32 = 1 << 22;
/// `DTE_GV` (dw1).
pub const DTE_GV: u32 = 1 << 23;
/// `DTE_IR` (dw1).
pub const DTE_IR: u32 = 1 << 29;
/// `DTE_IW` (dw1).
pub const DTE_IW: u32 = 1 << 30;

/// `DTE_DID_MASK` (dw2).
pub const DTE_DID_MASK: u32 = 0xFFFF;

/// `DTE_IV` (dw3).
pub const DTE_IV: u32 = 1 << 0;
/// `DTE_SE`.
pub const DTE_SE: u32 = 1 << 1;
/// `DTE_SA`.
pub const DTE_SA: u32 = 1 << 2;
/// `DTE_INTTABLEN_SHIFT`.
pub const DTE_INTTABLEN_SHIFT: u32 = 1;
/// `DTE_INTTABLEN_MASK`.
pub const DTE_INTTABLEN_MASK: u32 = 0xF;
/// `DTE_IRTP_MASK`.
pub const DTE_IRTP_MASK: u64 = 0x000F_FFFF_FFFF_FFC0;

/// `PTE_LVL5`.
pub const PTE_LVL5: u32 = 48;
/// `PTE_LVL4`.
pub const PTE_LVL4: u32 = 39;
/// `PTE_LVL3`.
pub const PTE_LVL3: u32 = 30;
/// `PTE_LVL2`.
pub const PTE_LVL2: u32 = 21;
/// `PTE_LVL1`.
pub const PTE_LVL1: u32 = 12;

/// `PTE_PADDR_MASK`.
pub const PTE_PADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
/// `PTE_IR`.
pub const PTE_IR: u64 = 1 << 61;
/// `PTE_IW`.
pub const PTE_IW: u64 = 1 << 62;

/// `DTE_GCR312_MASK`.
pub const DTE_GCR312_MASK: u32 = 0x3;
/// `DTE_GCR312_SHIFT`.
pub const DTE_GCR312_SHIFT: u32 = 24;

/// `DTE_GCR315_MASK`.
pub const DTE_GCR315_MASK: u32 = 0xFFFF;
/// `DTE_GCR315_SHIFT`.
pub const DTE_GCR315_SHIFT: u32 = 16;

/// `DTE_GCR331_MASK`.
pub const DTE_GCR331_MASK: u32 = 0xFFFFF;
/// `DTE_GCR331_SHIFT`.
pub const DTE_GCR331_SHIFT: u32 = 12;

/// `CMD_SHIFT`: where a command's opcode sits in `dw1`.
pub const CMD_SHIFT: u32 = 28;

/// `COMPLETION_WAIT`.
pub const COMPLETION_WAIT: u32 = 0x01;
/// `INVALIDATE_DEVTAB_ENTRY`.
pub const INVALIDATE_DEVTAB_ENTRY: u32 = 0x02;
/// `INVALIDATE_IOMMU_PAGES`.
pub const INVALIDATE_IOMMU_PAGES: u32 = 0x03;
/// `INVALIDATE_IOTLB_PAGES`.
pub const INVALIDATE_IOTLB_PAGES: u32 = 0x04;
/// `INVALIDATE_INTERRUPT_TABLE`.
pub const INVALIDATE_INTERRUPT_TABLE: u32 = 0x05;
/// `PREFETCH_IOMMU_PAGES`.
pub const PREFETCH_IOMMU_PAGES: u32 = 0x06;
/// `COMPLETE_PPR_REQUEST`.
pub const COMPLETE_PPR_REQUEST: u32 = 0x07;
/// `INVALIDATE_IOMMU_ALL`.
pub const INVALIDATE_IOMMU_ALL: u32 = 0x08;

/// `EVT_TYPE_SHIFT`.
pub const EVT_TYPE_SHIFT: u32 = 28;
/// `EVT_TYPE_MASK`.
pub const EVT_TYPE_MASK: u32 = 0xF;
/// `EVT_SID_SHIFT`.
pub const EVT_SID_SHIFT: u32 = 0;
/// `EVT_SID_MASK`.
pub const EVT_SID_MASK: u32 = 0xFFFF;
/// `EVT_DID_SHIFT`.
pub const EVT_DID_SHIFT: u32 = 0;
/// `EVT_DID_MASK`.
pub const EVT_DID_MASK: u32 = 0xFFFF;
/// `EVT_FLAG_SHIFT`.
pub const EVT_FLAG_SHIFT: u32 = 16;
/// `EVT_FLAG_MASK`.
pub const EVT_FLAG_MASK: u32 = 0xFFF;

/// `ILLEGAL_DEV_TABLE_ENTRY`: an IOMMU fault reason.
pub const ILLEGAL_DEV_TABLE_ENTRY: u32 = 0x1;
/// `IO_PAGE_FAULT`.
pub const IO_PAGE_FAULT: u32 = 0x2;
/// `DEV_TAB_HARDWARE_ERROR`.
pub const DEV_TAB_HARDWARE_ERROR: u32 = 0x3;
/// `PAGE_TAB_HARDWARE_ERROR`.
pub const PAGE_TAB_HARDWARE_ERROR: u32 = 0x4;
/// `ILLEGAL_COMMAND_ERROR`.
pub const ILLEGAL_COMMAND_ERROR: u32 = 0x5;
/// `COMMAND_HARDWARE_ERROR`.
pub const COMMAND_HARDWARE_ERROR: u32 = 0x6;
/// `IOTLB_INV_TIMEOUT`.
pub const IOTLB_INV_TIMEOUT: u32 = 0x7;
/// `INVALID_DEVICE_REQUEST`.
pub const INVALID_DEVICE_REQUEST: u32 = 0x8;

/// `EVT_GN`.
pub const EVT_GN: u32 = 1 << 16;
/// `EVT_NX`.
pub const EVT_NX: u32 = 1 << 17;
/// `EVT_US`.
pub const EVT_US: u32 = 1 << 18;
/// `EVT_I`.
pub const EVT_I: u32 = 1 << 19;
/// `EVT_PR`.
pub const EVT_PR: u32 = 1 << 20;
/// `EVT_RW`.
pub const EVT_RW: u32 = 1 << 21;
/// `EVT_PE`.
pub const EVT_PE: u32 = 1 << 22;
/// `EVT_RZ`.
pub const EVT_RZ: u32 = 1 << 23;
/// `EVT_TR`.
pub const EVT_TR: u32 = 1 << 24;

/// `PTE_NXTLVL(x)`: the next level's number in a page table entry.
pub const fn pte_nxtlvl(x: u64) -> u64 {
    (x & 0x7) << 9
}

/// `struct ivhd_dte`: a DEVICE TABLE ENTRY, the mapping of one bus-device-function.
///
/// ```text
///  0       Valid (V)
///  1       Translation Valid (TV)
///  7:8     Host Address Dirty (HAD)
///  9:11    Page Table Depth (usually 4)
///  12:51   Page Table Physical Address
///  52      PPR Enable
///  53      GPRP
///  54      Guest I/O Protection Valid (GIoV)
///  55      Guest Translation Valid (GV)
///  56:57   Guest Levels translated (GLX)
///  58:60   Guest CR3 bits 12:14 (GCR3TRP)
///  61      I/O Read Permission (IR)
///  62      I/O Write Permission (IW)
///  64:79   Domain ID
///  80:95   Guest CR3 bits 15:30 (GCR3TRP)
///  96      IOTLB Enable (I)
///  97      Suppress multiple I/O page faults (I)
///  98      Suppress all I/O page faults (SA)
///  99:100  Port I/O Control (IoCTL)
///  101     Cache IOTLB Hint
///  102     Snoop Disable (SD)
///  103     Allow Exclusion (EX)
///  104:105 System Management Message (SysMgt)
///  107:127 Guest CR3 bits 31:51 (GCR3TRP)
///  128     Interrupt Map Valid (IV)
///  129:132 Interrupt Table Length (IntTabLen)
/// ```
#[repr(C)]
pub struct IvhdDte {
    /// `dw0`..`dw7`, two to a word (see the module's deviations).
    q: [AtomicU64; 4],
}

impl IvhdDte {
    /// An all-zero (invalid) entry.
    pub const fn new() -> Self {
        Self {
            q: [const { AtomicU64::new(0) }; 4],
        }
    }

    /// `dte->dw<i>`.
    pub fn dw(&self, i: usize) -> u32 {
        (self.q[i / 2].load(Ordering::Relaxed) >> (32 * (i % 2))) as u32
    }

    /// `dte->dw<i> = v`.
    pub fn set_dw(&self, i: usize, v: u32) {
        let shift = 32 * (i % 2);
        let w = &self.q[i / 2];
        let old = w.load(Ordering::Relaxed);
        w.store(
            (old & !(0xFFFF_FFFFu64 << shift)) | (u64::from(v) << shift),
            Ordering::Relaxed,
        );
    }

    /// `_get64(&dte->dw<i>)`, `i` even.
    pub fn get64(&self, i: usize) -> u64 {
        self.q[i / 2].load(Ordering::Relaxed)
    }

    /// `_put64(&dte->dw<i>, v)`, `i` even.
    pub fn put64(&self, i: usize, v: u64) {
        self.q[i / 2].store(v, Ordering::Relaxed);
    }
}

impl Default for IvhdDte {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct ivhd_command`: one entry of the command buffer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IvhdCommand {
    /// `dw0`.
    pub dw0: u32,
    /// `dw1`.
    pub dw1: u32,
    /// `dw2`.
    pub dw2: u32,
    /// `dw3`.
    pub dw3: u32,
}

/// `struct ivhd_event`: one entry of the event log.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IvhdEvent {
    /// `dw0`.
    pub dw0: u32,
    /// `dw1`.
    pub dw1: u32,
    /// `dw2`.
    pub dw2: u32,
    /// `dw3`.
    pub dw3: u32,
}

/// `dte_set_guest_cr3(dte, paddr)`: Set Guest CR3 address.
pub fn dte_set_guest_cr3(dte: &IvhdDte, paddr: u64) {
    dte.set_dw(
        1,
        iommu_rmw32(
            dte.dw(1),
            DTE_GCR312_MASK,
            DTE_GCR312_SHIFT,
            (paddr >> 12) as u32,
        ),
    );
    dte.set_dw(
        2,
        iommu_rmw32(
            dte.dw(2),
            DTE_GCR315_MASK,
            DTE_GCR315_SHIFT,
            (paddr >> 15) as u32,
        ),
    );
    dte.set_dw(
        3,
        iommu_rmw32(
            dte.dw(3),
            DTE_GCR331_MASK,
            DTE_GCR331_SHIFT,
            (paddr >> 31) as u32,
        ),
    );
}

/// `dte_set_interrupt_table_root_ptr(dte, paddr)`: Set Interrupt Remapping Root Pointer.
pub fn dte_set_interrupt_table_root_ptr(dte: &IvhdDte, paddr: u64) {
    let ov = dte.get64(4);
    dte.put64(4, (ov & !DTE_IRTP_MASK) | (paddr & DTE_IRTP_MASK));
}

/// `dte_set_interrupt_table_length(dte, nEnt)`: Set Interrupt Remapping Table length.
pub fn dte_set_interrupt_table_length(dte: &IvhdDte, n_ent: u32) {
    dte.set_dw(
        4,
        iommu_rmw32(dte.dw(4), DTE_INTTABLEN_MASK, DTE_INTTABLEN_SHIFT, n_ent),
    );
}

/// `dte_set_interrupt_valid(dte)`: Set Interrupt Remapping Valid.
pub fn dte_set_interrupt_valid(dte: &IvhdDte) {
    dte.set_dw(4, dte.dw(4) | DTE_IV);
}

/// `dte_set_domain(dte, did)`: Set Domain ID in Device Table Entry.
pub fn dte_set_domain(dte: &IvhdDte, did: u16) {
    dte.set_dw(
        2,
        (dte.dw(2) & !DTE_DID_MASK) | (u32::from(did) & DTE_DID_MASK),
    );
}

/// `dte_set_host_page_table_root_ptr(dte, paddr)`: Set Page Table Pointer for device.
pub fn dte_set_host_page_table_root_ptr(dte: &IvhdDte, paddr: u64) {
    let mut ov = dte.get64(0) & !DTE_HPTRP_MASK;
    ov |= (paddr & DTE_HPTRP_MASK) | PTE_IW | PTE_IR;

    dte.put64(0, ov);
}

/// `dte_set_mode(dte, mode)`: Set Page Table Levels Mask.
pub fn dte_set_mode(dte: &IvhdDte, mode: u32) {
    dte.set_dw(
        0,
        iommu_rmw32(dte.dw(0), DTE_LEVEL_MASK, DTE_LEVEL_SHIFT, mode),
    );
}

/// `dte_set_tv(dte)`.
pub fn dte_set_tv(dte: &IvhdDte) {
    dte.set_dw(0, dte.dw(0) | DTE_TV);
}

/// `dte_set_valid(dte)`: Set Device Table Entry valid. Domain/Level/Mode/PageTable should
/// already be set.
pub fn dte_set_valid(dte: &IvhdDte) {
    dte.set_dw(0, dte.dw(0) | DTE_V);
}

/// `dte_is_valid(dte)`: Check if Device Table Entry is valid.
pub fn dte_is_valid(dte: &IvhdDte) -> bool {
    dte.dw(0) & DTE_V != 0
}

const _: () = {
    assert!(size_of::<IvhdDte>() == 32);
    assert!(size_of::<IvhdCommand>() == 16);
    assert!(size_of::<IvhdEvent>() == 16);
    assert!(HWDTE_SIZE == 2 * 1024 * 1024);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_pair_into_the_64_bit_ones() {
        let dte = IvhdDte::new();
        dte.set_dw(0, 0x1111_1111);
        dte.set_dw(1, 0x2222_2222);
        dte.set_dw(7, 0xdead_beef);
        assert_eq!(dte.get64(0), 0x2222_2222_1111_1111);
        assert_eq!(dte.dw(7), 0xdead_beef);
        assert_eq!(dte.dw(6), 0);
        dte.put64(4, 0x5555_5555_4444_4444);
        assert_eq!((dte.dw(4), dte.dw(5)), (0x4444_4444, 0x5555_5555));
    }

    #[test]
    fn device_attach_sequence() {
        // acpidmar's domain_map_device for an AMD IOMMU.
        let dte = IvhdDte::new();
        assert!(!dte_is_valid(&dte));
        dte_set_host_page_table_root_ptr(&dte, 0x1234_5000);
        dte_set_domain(&dte, 0xfffe);
        dte_set_mode(&dte, 3);
        dte_set_tv(&dte);
        dte_set_valid(&dte);
        assert!(dte_is_valid(&dte));
        assert_eq!(dte.dw(0), 0x1234_5000 | (3 << 9) | DTE_TV | DTE_V);
        assert_eq!(dte.dw(1), DTE_IR | DTE_IW);
        assert_eq!(dte.dw(2), 0xfffe);
        // the pointer replaces the old one and keeps the low bits
        dte_set_host_page_table_root_ptr(&dte, 0xabc_d000);
        assert_eq!(dte.dw(0), 0xabc_d000 | (3 << 9) | DTE_TV | DTE_V);
    }

    #[test]
    fn guest_cr3_and_interrupt_fields() {
        let dte = IvhdDte::new();
        dte_set_guest_cr3(&dte, 0x8000_7000);
        assert_eq!(dte.dw(1) >> DTE_GCR312_SHIFT & DTE_GCR312_MASK, 0x7 & 0x3);
        assert_eq!(
            dte.dw(2) >> DTE_GCR315_SHIFT,
            (0x8000_7000u64 >> 15) as u32 & 0xFFFF
        );
        assert_eq!(dte.dw(3) >> DTE_GCR331_SHIFT, 1);
        dte_set_interrupt_table_root_ptr(&dte, 0x1_2345_6780);
        assert_eq!(dte.get64(4) & DTE_IRTP_MASK, 0x1_2345_6780);
        dte_set_interrupt_table_length(&dte, 0xb);
        assert_eq!(dte.dw(4) >> 1 & 0xF, 0xb);
        dte_set_interrupt_valid(&dte);
        assert_eq!(dte.dw(4) & DTE_IV, DTE_IV);
    }

    #[test]
    fn next_level_field() {
        assert_eq!(pte_nxtlvl(2), 0x400);
        assert_eq!(pte_nxtlvl(9), 0x200);
    }
}
/* </TESTS> */
