/*	$OpenBSD: nvmereg.h,v 1.16 2024/09/13 09:57:34 jmatthew Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2014 David Gwynne <dlg@openbsd.org>
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

//! `<dev/ic/nvmereg.h>`: the NVM Express controller registers, the submission and completion
//! queue entries, the admin and NVM command opcodes and the identify and log page data.
//!
//! Upstream: sys/dev/ic/nvmereg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros (`NVME_CAP_MPSMAX(_r)`, `NVME_CC_IOSQES(_v)`,
//!   `NVME_SQTDBL(_q, _s)`, `NVME_CQE_SC(_f)`, ...) are lower-case `const fn`s
//!   (`docs/C_TO_RUST.md`); `NVME_CAP_NSSRS` and `NVME_CAP_CQR`, which the C writes with
//!   `ISSET`, return `bool`.
//! - The structures are `__packed __aligned(8)` in C. Every one but `struct nvm_smart_health`
//!   has its members at their natural alignment, so they are `#[repr(C, align(8))]` (Rust
//!   does not combine `packed` with `align`) and their sizes and member offsets are asserted
//!   at compile time; `NvmSmartHealth`, whose `temperature` sits at offset 1, is
//!   `#[repr(C, packed)]`.
//! - The `entry` union of `struct nvme_sqe` and `struct nvme_sqe_io` is [`NvmeSqeEntry`];
//!   its PRP view is read and written through [`NvmeSqeEntry::prp`] and
//!   [`NvmeSqeEntry::set_prp`]. `struct nvme_sqe_q` and `struct nvme_sqe_io` are other
//!   views of the same 64 bytes, reached from an [`NvmeSqe`] through
//!   [`NvmeSqe::as_q_mut`] and [`NvmeSqe::as_io_mut`] where the C casts the pointer.
//! - The `%b` format strings (`NVM_ID_CTRL_CTRATT_FMT`, ...) are byte strings with the C's
//!   octal escapes written in hexadecimal.

/// `NVME_CAP`: Controller Capabilities.
pub const NVME_CAP: usize = 0x0000;
/// `NVME_CAP_MPSMAX(_r)`: the largest memory page size, as a shift.
pub const fn nvme_cap_mpsmax(r: u64) -> u32 {
    12 + ((r >> 52) & 0xf) as u32
}
/// `NVME_CAP_MPSMIN(_r)`: the smallest memory page size, as a shift.
pub const fn nvme_cap_mpsmin(r: u64) -> u32 {
    12 + ((r >> 48) & 0xf) as u32
}
/// `NVME_CAP_CSS(_r)`: the command sets supported.
pub const fn nvme_cap_css(r: u64) -> u64 {
    (r >> 37) & 0x7f
}
/// `NVME_CAP_CSS_NVM`.
pub const NVME_CAP_CSS_NVM: u64 = 1 << 0;
/// `NVME_CAP_NSSRS(_r)`: NVM subsystem reset supported.
pub const fn nvme_cap_nssrs(r: u64) -> bool {
    r & (1u64 << 36) != 0
}
/// `NVME_CAP_DSTRD(_r)`: the doorbell stride, in bytes.
pub const fn nvme_cap_dstrd(r: u64) -> u32 {
    1 << (2 + ((r >> 32) & 0xf))
}
/// `NVME_CAP_TO(_r)`: the ready timeout, in milliseconds.
pub const fn nvme_cap_to(r: u64) -> u64 {
    500 * ((r >> 24) & 0xff)
}
/// `NVME_CAP_AMS(_r)`: the arbitration mechanisms supported.
pub const fn nvme_cap_ams(r: u64) -> u64 {
    (r >> 17) & 0x3
}
/// `NVME_CAP_AMS_WRR`.
pub const NVME_CAP_AMS_WRR: u64 = 1 << 0;
/// `NVME_CAP_AMS_VENDOR`.
pub const NVME_CAP_AMS_VENDOR: u64 = 1 << 1;
/// `NVME_CAP_CQR(_r)`: contiguous queues required.
pub const fn nvme_cap_cqr(r: u64) -> bool {
    r & (1 << 16) != 0
}
/// `NVME_CAP_MQES(_r)`: the largest queue size.
pub const fn nvme_cap_mqes(r: u64) -> u64 {
    (r & 0xffff) + 1
}
/// `NVME_CAP_LO`.
pub const NVME_CAP_LO: usize = 0x0000;
/// `NVME_CAP_HI`.
pub const NVME_CAP_HI: usize = 0x0004;
/// `NVME_VS`: Version.
pub const NVME_VS: usize = 0x0008;
/// `NVME_VS_MJR(_r)`.
pub const fn nvme_vs_mjr(r: u32) -> u32 {
    (r & 0xffff_0000) >> 16
}
/// `NVME_VS_MNR(_r)`.
pub const fn nvme_vs_mnr(r: u32) -> u32 {
    (r & 0x0000_ff00) >> 8
}
/// `NVME_INTMS`: Interrupt Mask Set.
pub const NVME_INTMS: usize = 0x000c;
/// `NVME_INTMC`: Interrupt Mask Clear.
pub const NVME_INTMC: usize = 0x0010;
/// `NVME_CC`: Controller Configuration.
pub const NVME_CC: usize = 0x0014;
/// `NVME_CC_IOCQES(_v)`.
pub const fn nvme_cc_iocqes(v: u32) -> u32 {
    (v & 0xf) << 20
}
/// `NVME_CC_IOCQES_MASK`.
pub const NVME_CC_IOCQES_MASK: u32 = nvme_cc_iocqes(0xf);
/// `NVME_CC_IOCQES_R(_v)`.
pub const fn nvme_cc_iocqes_r(v: u32) -> u32 {
    (v >> 20) & 0xf
}
/// `NVME_CC_IOSQES(_v)`.
pub const fn nvme_cc_iosqes(v: u32) -> u32 {
    (v & 0xf) << 16
}
/// `NVME_CC_IOSQES_MASK`.
pub const NVME_CC_IOSQES_MASK: u32 = nvme_cc_iosqes(0xf);
/// `NVME_CC_IOSQES_R(_v)`.
pub const fn nvme_cc_iosqes_r(v: u32) -> u32 {
    (v >> 16) & 0xf
}
/// `NVME_CC_SHN(_v)`.
pub const fn nvme_cc_shn(v: u32) -> u32 {
    (v & 0x3) << 14
}
/// `NVME_CC_SHN_MASK`.
pub const NVME_CC_SHN_MASK: u32 = nvme_cc_shn(0x3);
/// `NVME_CC_SHN_R(_v)` (the C shifts by 15, not 14).
pub const fn nvme_cc_shn_r(v: u32) -> u32 {
    (v >> 15) & 0x3
}
/// `NVME_CC_SHN_NONE`.
pub const NVME_CC_SHN_NONE: u32 = 0;
/// `NVME_CC_SHN_NORMAL`.
pub const NVME_CC_SHN_NORMAL: u32 = 1;
/// `NVME_CC_SHN_ABRUPT`.
pub const NVME_CC_SHN_ABRUPT: u32 = 2;
/// `NVME_CC_AMS(_v)`.
pub const fn nvme_cc_ams(v: u32) -> u32 {
    (v & 0x7) << 11
}
/// `NVME_CC_AMS_MASK`.
pub const NVME_CC_AMS_MASK: u32 = nvme_cc_ams(0x7);
/// `NVME_CC_AMS_R(_v)`.
pub const fn nvme_cc_ams_r(v: u32) -> u32 {
    (v >> 11) & 0xf
}
/// `NVME_CC_AMS_RR`: round-robin.
pub const NVME_CC_AMS_RR: u32 = 0;
/// `NVME_CC_AMS_WRR_U`: weighted round-robin w/ urgent.
pub const NVME_CC_AMS_WRR_U: u32 = 1;
/// `NVME_CC_AMS_VENDOR`: vendor.
pub const NVME_CC_AMS_VENDOR: u32 = 7;
/// `NVME_CC_MPS(_v)`: the memory page size, from its shift.
pub const fn nvme_cc_mps(v: u32) -> u32 {
    (v.wrapping_sub(12) & 0xf) << 7
}
/// `NVME_CC_MPS_MASK`.
pub const NVME_CC_MPS_MASK: u32 = 0xf << 7;
/// `NVME_CC_MPS_R(_v)`.
pub const fn nvme_cc_mps_r(v: u32) -> u32 {
    12 + ((v >> 7) & 0xf)
}
/// `NVME_CC_CSS(_v)`.
pub const fn nvme_cc_css(v: u32) -> u32 {
    (v & 0x7) << 4
}
/// `NVME_CC_CSS_MASK`.
pub const NVME_CC_CSS_MASK: u32 = nvme_cc_css(0x7);
/// `NVME_CC_CSS_R(_v)`.
pub const fn nvme_cc_css_r(v: u32) -> u32 {
    (v >> 4) & 0x7
}
/// `NVME_CC_CSS_NVM`.
pub const NVME_CC_CSS_NVM: u32 = 0;
/// `NVME_CC_EN`.
pub const NVME_CC_EN: u32 = 1 << 0;
/// `NVME_CSTS`: Controller Status.
pub const NVME_CSTS: usize = 0x001c;
/// `NVME_CSTS_SHST_MASK`.
pub const NVME_CSTS_SHST_MASK: u32 = 0x3 << 2;
/// `NVME_CSTS_SHST_NONE`: normal operation.
pub const NVME_CSTS_SHST_NONE: u32 = 0x0 << 2;
/// `NVME_CSTS_SHST_WAIT`: shutdown processing occurring.
pub const NVME_CSTS_SHST_WAIT: u32 = 0x1 << 2;
/// `NVME_CSTS_SHST_DONE`: shutdown processing complete.
pub const NVME_CSTS_SHST_DONE: u32 = 0x2 << 2;
/// `NVME_CSTS_CFS`.
pub const NVME_CSTS_CFS: u32 = 1 << 1;
/// `NVME_CSTS_RDY`.
pub const NVME_CSTS_RDY: u32 = 1 << 0;
/// `NVME_NSSR`: NVM Subsystem Reset (Optional).
pub const NVME_NSSR: usize = 0x0020;
/// `NVME_AQA`: Admin Queue Attributes.
pub const NVME_AQA: usize = 0x0024;
/// `NVME_AQA_ACQS(_v)`: Admin Completion Queue Size.
pub const fn nvme_aqa_acqs(v: u32) -> u32 {
    (v - 1) << 16
}
/// `NVME_AQA_ASQS(_v)`: Admin Submission Queue Size.
pub const fn nvme_aqa_asqs(v: u32) -> u32 {
    v - 1
}
/// `NVME_ASQ`: Admin Submission Queue Base Address.
pub const NVME_ASQ: usize = 0x0028;
/// `NVME_ACQ`: Admin Completion Queue Base Address.
pub const NVME_ACQ: usize = 0x0030;

/// `NVME_ADMIN_Q`.
pub const NVME_ADMIN_Q: u16 = 0;
/// `NVME_SQTDBL(_q, _s)`: Submission Queue Tail Doorbell.
pub const fn nvme_sqtdbl(q: u16, s: u32) -> usize {
    0x1000 + (2 * q as usize) * s as usize
}
/// `NVME_CQHDBL(_q, _s)`: Completion Queue Head Doorbell.
pub const fn nvme_cqhdbl(q: u16, s: u32) -> usize {
    0x1000 + (2 * q as usize + 1) * s as usize
}

/// `struct nvme_sge`.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default)]
pub struct NvmeSge {
    /// `id`.
    pub id: u8,
    /// `_reserved`.
    pub _reserved: [u8; 15],
}

/// `struct nvme_sge_data`.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default)]
pub struct NvmeSgeData {
    /// `id`.
    pub id: u8,
    /// `_reserved`.
    pub _reserved: [u8; 3],
    /// `length`.
    pub length: u32,
    /// `address`.
    pub address: u64,
}

/// `struct nvme_sge_bit_bucket`.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default)]
pub struct NvmeSgeBitBucket {
    /// `id`.
    pub id: u8,
    /// `_reserved`.
    pub _reserved: [u8; 3],
    /// `length`.
    pub length: u32,
    /// `address`.
    pub address: u64,
}

/// The `entry` union of `struct nvme_sqe` and `struct nvme_sqe_io`: two PRP entries or an
/// SGL descriptor.
#[repr(C)]
#[derive(Clone, Copy)]
pub union NvmeSqeEntry {
    /// `prp`: physical region page entries, little-endian.
    pub prp: [u64; 2],
    /// `sge`.
    pub sge: NvmeSge,
}

impl NvmeSqeEntry {
    /// Both views zero.
    pub const fn zeroed() -> Self {
        Self { prp: [0; 2] }
    }

    /// `entry.prp[i]`, as stored (little-endian).
    pub fn prp(&self, i: usize) -> u64 {
        // SAFETY: both members are 16 bytes of plain integers, so every bit pattern is a
        // valid `[u64; 2]`.
        unsafe { self.prp[i] }
    }

    /// `htolem64(&entry.prp[i], v)`.
    pub fn set_prp(&mut self, i: usize, v: u64) {
        // SAFETY: as for `prp`; writing one element leaves the union valid either way.
        unsafe { self.prp[i] = v.to_le() };
    }
}

impl core::fmt::Debug for NvmeSqeEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NvmeSqeEntry")
            .field("prp", &[self.prp(0), self.prp(1)])
            .finish()
    }
}

/// `struct nvme_sqe`: a submission queue entry.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct NvmeSqe {
    /// `opcode`.
    pub opcode: u8,
    /// `flags`.
    pub flags: u8,
    /// `cid`.
    pub cid: u16,
    /// `nsid`.
    pub nsid: u32,
    /// `_reserved`.
    pub _reserved: [u8; 8],
    /// `mptr`.
    pub mptr: u64,
    /// `entry`.
    pub entry: NvmeSqeEntry,
    /// `cdw10`.
    pub cdw10: u32,
    /// `cdw11`.
    pub cdw11: u32,
    /// `cdw12`.
    pub cdw12: u32,
    /// `cdw13`.
    pub cdw13: u32,
    /// `cdw14`.
    pub cdw14: u32,
    /// `cdw15`.
    pub cdw15: u32,
}

impl NvmeSqe {
    /// `memset(&sqe, 0, sizeof(sqe))`.
    pub const fn zeroed() -> Self {
        Self {
            opcode: 0,
            flags: 0,
            cid: 0,
            nsid: 0,
            _reserved: [0; 8],
            mptr: 0,
            entry: NvmeSqeEntry::zeroed(),
            cdw10: 0,
            cdw11: 0,
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    /// `(struct nvme_sqe_q *)sqe`: the queue-creation view of the entry.
    pub fn as_q_mut(&mut self) -> &mut NvmeSqeQ {
        // SAFETY: both types are 64 bytes of plain integers with alignment 8 (asserted
        // below), so every bit pattern of one is a valid other, and the borrow is exclusive.
        unsafe { &mut *core::ptr::from_mut(self).cast::<NvmeSqeQ>() }
    }

    /// `(struct nvme_sqe_io *)sqe`: the I/O command view of the entry.
    pub fn as_io_mut(&mut self) -> &mut NvmeSqeIo {
        // SAFETY: as for `as_q_mut`.
        unsafe { &mut *core::ptr::from_mut(self).cast::<NvmeSqeIo>() }
    }
}

/// `struct nvme_sqe_q`: the entry of a queue creation or deletion command.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default)]
pub struct NvmeSqeQ {
    /// `opcode`.
    pub opcode: u8,
    /// `flags`.
    pub flags: u8,
    /// `cid`.
    pub cid: u16,
    /// `_reserved1`.
    pub _reserved1: [u8; 20],
    /// `prp1`.
    pub prp1: u64,
    /// `_reserved2`.
    pub _reserved2: [u8; 8],
    /// `qid`.
    pub qid: u16,
    /// `qsize`.
    pub qsize: u16,
    /// `qflags`: `NVM_SQE_*`.
    pub qflags: u8,
    /// `_reserved3`.
    pub _reserved3: u8,
    /// `cqid`: XXX interrupt vector for cq.
    pub cqid: u16,
    /// `_reserved4`.
    pub _reserved4: [u8; 16],
}

/// `NVM_SQE_SQ_QPRIO_URG`.
pub const NVM_SQE_SQ_QPRIO_URG: u8 = 0x0 << 1;
/// `NVM_SQE_SQ_QPRIO_HI`.
pub const NVM_SQE_SQ_QPRIO_HI: u8 = 0x1 << 1;
/// `NVM_SQE_SQ_QPRIO_MED`.
pub const NVM_SQE_SQ_QPRIO_MED: u8 = 0x2 << 1;
/// `NVM_SQE_SQ_QPRIO_LOW`.
pub const NVM_SQE_SQ_QPRIO_LOW: u8 = 0x3 << 1;
/// `NVM_SQE_CQ_IEN`.
pub const NVM_SQE_CQ_IEN: u8 = 1 << 1;
/// `NVM_SQE_Q_PC`.
pub const NVM_SQE_Q_PC: u8 = 1 << 0;

/// `struct nvme_sqe_io`: the entry of an NVM read or write.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct NvmeSqeIo {
    /// `opcode`.
    pub opcode: u8,
    /// `flags`.
    pub flags: u8,
    /// `cid`.
    pub cid: u16,
    /// `nsid`.
    pub nsid: u32,
    /// `_reserved`.
    pub _reserved: [u8; 8],
    /// `mptr`.
    pub mptr: u64,
    /// `entry`.
    pub entry: NvmeSqeEntry,
    /// `slba`: Starting LBA.
    pub slba: u64,
    /// `nlb`: Number of Logical Blocks.
    pub nlb: u16,
    /// `ioflags`.
    pub ioflags: u16,
    /// `dsm`: Dataset Management.
    pub dsm: u8,
    /// `_reserved2`.
    pub _reserved2: [u8; 3],
    /// `eilbrt`: Expected Initial Logical Block Reference Tag.
    pub eilbrt: u32,
    /// `elbat`: Expected Logical Block Application Tag.
    pub elbat: u16,
    /// `elbatm`: Expected Logical Block Application Tag Mask.
    pub elbatm: u16,
}

/// `struct nvme_cqe`: a completion queue entry.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default)]
pub struct NvmeCqe {
    /// `cdw0`.
    pub cdw0: u32,
    /// `_reserved`.
    pub _reserved: u32,
    /// `sqhd`: SQ Head Pointer.
    pub sqhd: u16,
    /// `sqid`: SQ Identifier.
    pub sqid: u16,
    /// `cid`: Command Identifier.
    pub cid: u16,
    /// `flags`: `NVME_CQE_*`.
    pub flags: u16,
}

/// `NVME_CQE_DNR`.
pub const NVME_CQE_DNR: u16 = 1 << 15;
/// `NVME_CQE_M`.
pub const NVME_CQE_M: u16 = 1 << 14;
/// `NVME_CQE_SCT(_f)`: the status code type.
pub const fn nvme_cqe_sct(f: u16) -> u16 {
    f & (0x07 << 9)
}
/// `NVME_CQE_SCT_GENERIC`.
pub const NVME_CQE_SCT_GENERIC: u16 = 0x00 << 9;
/// `NVME_CQE_SCT_COMMAND`.
pub const NVME_CQE_SCT_COMMAND: u16 = 0x01 << 9;
/// `NVME_CQE_SCT_MEDIAERR`.
pub const NVME_CQE_SCT_MEDIAERR: u16 = 0x02 << 9;
/// `NVME_CQE_SCT_VENDOR`.
pub const NVME_CQE_SCT_VENDOR: u16 = 0x07 << 9;
/// `NVME_CQE_SC(_f)`: the status code.
pub const fn nvme_cqe_sc(f: u16) -> u16 {
    f & (0xff << 1)
}
/// `NVME_CQE_SC_SUCCESS`.
pub const NVME_CQE_SC_SUCCESS: u16 = 0x00 << 1;
/// `NVME_CQE_SC_INVALID_OPCODE`.
pub const NVME_CQE_SC_INVALID_OPCODE: u16 = 0x01 << 1;
/// `NVME_CQE_SC_INVALID_FIELD`.
pub const NVME_CQE_SC_INVALID_FIELD: u16 = 0x02 << 1;
/// `NVME_CQE_SC_CID_CONFLICT`.
pub const NVME_CQE_SC_CID_CONFLICT: u16 = 0x03 << 1;
/// `NVME_CQE_SC_DATA_XFER_ERR`.
pub const NVME_CQE_SC_DATA_XFER_ERR: u16 = 0x04 << 1;
/// `NVME_CQE_SC_ABRT_BY_NO_PWR`.
pub const NVME_CQE_SC_ABRT_BY_NO_PWR: u16 = 0x05 << 1;
/// `NVME_CQE_SC_INTERNAL_DEV_ERR`.
pub const NVME_CQE_SC_INTERNAL_DEV_ERR: u16 = 0x06 << 1;
/// `NVME_CQE_SC_CMD_ABRT_REQD`.
pub const NVME_CQE_SC_CMD_ABRT_REQD: u16 = 0x07 << 1;
/// `NVME_CQE_SC_CMD_ABDR_SQ_DEL`.
pub const NVME_CQE_SC_CMD_ABDR_SQ_DEL: u16 = 0x08 << 1;
/// `NVME_CQE_SC_CMD_ABDR_FUSE_ERR`.
pub const NVME_CQE_SC_CMD_ABDR_FUSE_ERR: u16 = 0x09 << 1;
/// `NVME_CQE_SC_CMD_ABDR_FUSE_MISS`.
pub const NVME_CQE_SC_CMD_ABDR_FUSE_MISS: u16 = 0x0a << 1;
/// `NVME_CQE_SC_INVALID_NS`.
pub const NVME_CQE_SC_INVALID_NS: u16 = 0x0b << 1;
/// `NVME_CQE_SC_CMD_SEQ_ERR`.
pub const NVME_CQE_SC_CMD_SEQ_ERR: u16 = 0x0c << 1;
/// `NVME_CQE_SC_INVALID_LAST_SGL`.
pub const NVME_CQE_SC_INVALID_LAST_SGL: u16 = 0x0d << 1;
/// `NVME_CQE_SC_INVALID_NUM_SGL`.
pub const NVME_CQE_SC_INVALID_NUM_SGL: u16 = 0x0e << 1;
/// `NVME_CQE_SC_DATA_SGL_LEN`.
pub const NVME_CQE_SC_DATA_SGL_LEN: u16 = 0x0f << 1;
/// `NVME_CQE_SC_MDATA_SGL_LEN`.
pub const NVME_CQE_SC_MDATA_SGL_LEN: u16 = 0x10 << 1;
/// `NVME_CQE_SC_SGL_TYPE_INVALID`.
pub const NVME_CQE_SC_SGL_TYPE_INVALID: u16 = 0x11 << 1;
/// `NVME_CQE_SC_LBA_RANGE`.
pub const NVME_CQE_SC_LBA_RANGE: u16 = 0x80 << 1;
/// `NVME_CQE_SC_CAP_EXCEEDED`.
pub const NVME_CQE_SC_CAP_EXCEEDED: u16 = 0x81 << 1;
/// `NVME_CQE_NS_NOT_RDY`.
pub const NVME_CQE_NS_NOT_RDY: u16 = 0x82 << 1;
/// `NVME_CQE_RSV_CONFLICT`.
pub const NVME_CQE_RSV_CONFLICT: u16 = 0x83 << 1;
/// `NVME_CQE_PHASE`.
pub const NVME_CQE_PHASE: u16 = 1 << 0;

/// `NVM_ADMIN_DEL_IOSQ`: Delete I/O Submission Queue.
pub const NVM_ADMIN_DEL_IOSQ: u8 = 0x00;
/// `NVM_ADMIN_ADD_IOSQ`: Create I/O Submission Queue.
pub const NVM_ADMIN_ADD_IOSQ: u8 = 0x01;
/// `NVM_ADMIN_GET_LOG_PG`: Get Log Page.
pub const NVM_ADMIN_GET_LOG_PG: u8 = 0x02;
/// `NVM_ADMIN_DEL_IOCQ`: Delete I/O Completion Queue.
pub const NVM_ADMIN_DEL_IOCQ: u8 = 0x04;
/// `NVM_ADMIN_ADD_IOCQ`: Create I/O Completion Queue.
pub const NVM_ADMIN_ADD_IOCQ: u8 = 0x05;
/// `NVM_ADMIN_IDENTIFY`: Identify.
pub const NVM_ADMIN_IDENTIFY: u8 = 0x06;
/// `NVM_ADMIN_ABORT`: Abort.
pub const NVM_ADMIN_ABORT: u8 = 0x08;
/// `NVM_ADMIN_SET_FEATURES`: Set Features.
pub const NVM_ADMIN_SET_FEATURES: u8 = 0x09;
/// `NVM_ADMIN_GET_FEATURES`: Get Features.
pub const NVM_ADMIN_GET_FEATURES: u8 = 0x0a;
/// `NVM_ADMIN_ASYNC_EV_REQ`: Asynchronous Event Request.
pub const NVM_ADMIN_ASYNC_EV_REQ: u8 = 0x0c;
/// `NVM_ADMIN_FW_ACTIVATE`: Firmware Activate.
pub const NVM_ADMIN_FW_ACTIVATE: u8 = 0x10;
/// `NVM_ADMIN_FW_DOWNLOAD`: Firmware Image Download.
pub const NVM_ADMIN_FW_DOWNLOAD: u8 = 0x11;
/// `NVM_ADMIN_SELFTEST`: Start self test.
pub const NVM_ADMIN_SELFTEST: u8 = 0x14;

/// `NVM_CMD_FLUSH`: Flush.
pub const NVM_CMD_FLUSH: u8 = 0x00;
/// `NVM_CMD_WRITE`: Write.
pub const NVM_CMD_WRITE: u8 = 0x01;
/// `NVM_CMD_READ`: Read.
pub const NVM_CMD_READ: u8 = 0x02;
/// `NVM_CMD_WR_UNCOR`: Write Uncorrectable.
pub const NVM_CMD_WR_UNCOR: u8 = 0x04;
/// `NVM_CMD_COMPARE`: Compare.
pub const NVM_CMD_COMPARE: u8 = 0x05;
/// `NVM_CMD_DSM`: Dataset Management.
pub const NVM_CMD_DSM: u8 = 0x09;

/// `struct nvm_identify_psd`: Power State Descriptor Data.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default)]
pub struct NvmIdentifyPsd {
    /// `mp`: Max Power.
    pub mp: u16,
    /// `flags`.
    pub flags: u16,
    /// `enlat`: Entry Latency.
    pub enlat: u32,
    /// `exlat`: Exit Latency.
    pub exlat: u32,
    /// `rrt`: Relative Read Throughput.
    pub rrt: u8,
    /// `rrl`: Relative Read Latency.
    pub rrl: u8,
    /// `rwt`: Relative Write Throughput.
    pub rwt: u8,
    /// `rwl`: Relative Write Latency.
    pub rwl: u8,
    /// `_reserved`.
    pub _reserved: [u8; 16],
}

/// `struct nvm_identify_controller`: what Identify (CNS 1) returns. 4 KiB: never built on
/// the kernel stack, only read in place.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct NvmIdentifyController {
    // Controller Capabilities and Features
    /// `vid`: PCI Vendor ID.
    pub vid: u16,
    /// `ssvid`: PCI Subsystem Vendor ID.
    pub ssvid: u16,
    /// `sn`: Serial Number.
    pub sn: [u8; 20],
    /// `mn`: Model Number.
    pub mn: [u8; 40],
    /// `fr`: Firmware Revision.
    pub fr: [u8; 8],
    /// `rab`: Recommended Arbitration Burst.
    pub rab: u8,
    /// `ieee`: IEEE OUI Identifier.
    pub ieee: [u8; 3],
    /// `cmic`: Controller Multi-Path I/O and Namespace Sharing Capabilities.
    pub cmic: u8,
    /// `mdts`: Maximum Data Transfer Size.
    pub mdts: u8,
    /// `cntlid`: Controller ID.
    pub cntlid: u16,
    /// `_reserved1`.
    pub _reserved1: [u8; 16],
    /// `ctratt`.
    pub ctratt: u32,
    /// `_reserved9`.
    pub _reserved9: [u8; 156],

    // Admin Command Set Attributes & Optional Controller Capabilities
    /// `oacs`: Optional Admin Command Support.
    pub oacs: u16,
    /// `acl`: Abort Command Limit.
    pub acl: u8,
    /// `aerl`: Asynchronous Event Request Limit.
    pub aerl: u8,
    /// `frmw`: Firmware Updates.
    pub frmw: u8,
    /// `lpa`: Log Page Attributes.
    pub lpa: u8,
    /// `elpe`: Error Log Page Entries.
    pub elpe: u8,
    /// `npss`: Number of Power States Support.
    pub npss: u8,
    /// `avscc`: Admin Vendor Specific Command Configuration.
    pub avscc: u8,
    /// `apsta`: Autonomous Power State Transition Attributes.
    pub apsta: u8,
    /// `_reserved2`.
    pub _reserved2: [u8; 62],
    /// `sanicap`.
    pub sanicap: u32,
    /// `_reserved10`.
    pub _reserved10: [u8; 180],

    // NVM Command Set Attributes
    /// `sqes`: Submission Queue Entry Size.
    pub sqes: u8,
    /// `cqes`: Completion Queue Entry Size.
    pub cqes: u8,
    /// `_reserved3`.
    pub _reserved3: [u8; 2],
    /// `nn`: Number of Namespaces.
    pub nn: u32,
    /// `oncs`: Optional NVM Command Support.
    pub oncs: u16,
    /// `fuses`: Fused Operation Support.
    pub fuses: u16,
    /// `fna`: Format NVM Attributes.
    pub fna: u8,
    /// `vwc`: Volatile Write Cache.
    pub vwc: u8,
    /// `awun`: Atomic Write Unit Normal.
    pub awun: u16,
    /// `awupf`: Atomic Write Unit Power Fail.
    pub awupf: u16,
    /// `nvscc`: NVM Vendor Specific Command.
    pub nvscc: u8,
    /// `_reserved4`.
    pub _reserved4: [u8; 1],
    /// `acwu`: Atomic Compare & Write Unit.
    pub acwu: u16,
    /// `_reserved5`.
    pub _reserved5: [u8; 2],
    /// `sgls`: SGL Support.
    pub sgls: u32,
    /// `_reserved6`.
    pub _reserved6: [u8; 164],

    // I/O Command Set Attributes
    /// `_reserved7`.
    pub _reserved7: [u8; 1344],

    // Power State Descriptors
    /// `psd`: Power State Descriptors.
    pub psd: [NvmIdentifyPsd; 32],

    // Vendor Specific
    /// `_reserved8`.
    pub _reserved8: [u8; 1024],
}

/// `NVM_ID_CTRL_CTRATT_FMT`: `%b` names of the `ctratt` bits.
pub const NVM_ID_CTRL_CTRATT_FMT: &[u8] = b"\x10\
    \x0eDELEG\x0fDEVNVM\x10ELBAS\x05ENDURGRPS\
    \x0cFIXCAPMGMT\x01HOSTID\x0bMDS\x02NOPSPM\
    \x08NSGRAN\x03NVMSETS\x06PREDLATENCY\x04READRCVRY\
    \x09SQASSOC\x07TBKAS\x0aUUIDLIST\x0dVARCAPMGMT";
/// `NVM_ID_CTRL_OACS_FMT`: `%b` names of the `oacs` bits.
pub const NVM_ID_CTRL_OACS_FMT: &[u8] = b"\x10\
    \x0bCAFL\x09DBBC\x06DIREC\x05DST\x0aGLBAS\
    \x02FORMAT\x03FWCD\x07MISR\x04NSMGMT\x01SECSR\
    \x08VM";
/// `NVM_ID_CTRL_LPA_PE`.
pub const NVM_ID_CTRL_LPA_PE: u8 = 1 << 4;
/// `NVM_ID_CTRL_SANICAP_FMT`: `%b` names of the `sanicap` bits.
pub const NVM_ID_CTRL_SANICAP_FMT: &[u8] = b"\x10\
    \x02BlockErase\x01CryptoErase\x03Overwrite";
/// `NVM_ID_CTRL_ONCS_FMT`: `%b` names of the `oncs` bits.
pub const NVM_ID_CTRL_ONCS_FMT: &[u8] = b"\x10\
    \x06RSV\x01SCMP\x09SCPY\x03SDMGMT\x05SF\
    \x08SV\x02SWU\x04SWZ\x07TS";
/// `NVM_ID_CTRL_FNA_CRYPTOFORMAT`.
pub const NVM_ID_CTRL_FNA_CRYPTOFORMAT: u8 = 1 << 2;
/// `NVM_ID_CTRL_VWC_PRESENT`.
pub const NVM_ID_CTRL_VWC_PRESENT: u8 = 1 << 0;

/// `struct nvm_namespace_format`: one LBA format.
#[repr(C, align(4))]
#[derive(Clone, Copy, Debug, Default)]
pub struct NvmNamespaceFormat {
    /// `ms`: Metadata Size.
    pub ms: u16,
    /// `lbads`: LBA Data Size.
    pub lbads: u8,
    /// `rp`: Relative Performance.
    pub rp: u8,
}

/// `struct nvm_identify_namespace`: what Identify (CNS 0) returns for a namespace. 4 KiB:
/// never built on the kernel stack.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct NvmIdentifyNamespace {
    /// `nsze`: Namespace Size.
    pub nsze: u64,
    /// `ncap`: Namespace Capacity.
    pub ncap: u64,
    /// `nuse`: Namespace Utilization.
    pub nuse: u64,
    /// `nsfeat`: Namespace Features.
    pub nsfeat: u8,
    /// `nlbaf`: Number of LBA Formats.
    pub nlbaf: u8,
    /// `flbas`: Formatted LBA Size.
    pub flbas: u8,
    /// `mc`: Metadata Capabilities.
    pub mc: u8,
    /// `dpc`: End-to-end Data Protection Capabilities.
    pub dpc: u8,
    /// `dps`: End-to-end Data Protection Type Settings.
    pub dps: u8,
    /// `_reserved1`.
    pub _reserved1: [u8; 74],
    /// `nguid`.
    pub nguid: [u8; 16],
    /// `eui64`: BIG-endian.
    pub eui64: [u8; 8],
    /// `lbaf`: LBA Format Support.
    pub lbaf: [NvmNamespaceFormat; 16],
    /// `_reserved2`.
    pub _reserved2: [u8; 192],
    /// `vs`.
    pub vs: [u8; 3712],
}

/// `NVME_ID_NS_NSFEAT_THIN_PROV`.
pub const NVME_ID_NS_NSFEAT_THIN_PROV: u8 = 1 << 0;
/// `NVME_ID_NS_NSFEAT_FMT`: `%b` names of the `nsfeat` bits.
pub const NVME_ID_NS_NSFEAT_FMT: &[u8] = b"\x10\
    \x02NSABP\x05OPTPERF\x01THIN_PROV\x04UIDREUSE\x03DAE";
/// `NVME_ID_NS_FLBAS(_f)`: the formatted LBA size's index.
pub const fn nvme_id_ns_flbas(f: u8) -> u8 {
    f & 0x0f
}
/// `NVME_ID_NS_FLBAS_MD`.
pub const NVME_ID_NS_FLBAS_MD: u8 = 0x10;
/// `NVME_ID_NS_DPS_PIP`.
pub const NVME_ID_NS_DPS_PIP: u8 = 1 << 3;
/// `NVME_ID_NS_DPS_TYPE(_f)`.
pub const fn nvme_id_ns_dps_type(f: u8) -> u8 {
    f & 0x7
}

/// `NVM_LOG_PAGE_SMART_HEALTH`.
pub const NVM_LOG_PAGE_SMART_HEALTH: u32 = 0x02;

/// `struct nvm_smart_health`: the SMART / Health Information log page. Packed: `temperature`
/// sits at offset 1, so read the members by value (`{ h.temperature }`).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct NvmSmartHealth {
    /// `critical_warning`: `NVM_HEALTH_CW_*`.
    pub critical_warning: u8,
    /// `temperature`: Kelvin, little-endian.
    pub temperature: u16,
    /// `avail_spare`.
    pub avail_spare: u8,
    /// `avail_spare_threshold`.
    pub avail_spare_threshold: u8,
    /// `percent_used`.
    pub percent_used: u8,
    /// `end_grp_summary`: 1.4+.
    pub end_grp_summary: u8,
    /// `_reserved1`.
    pub _reserved1: [u8; 25],
    /// `data_units_read`.
    pub data_units_read: [u64; 2],
    /// `data_units_written`.
    pub data_units_written: [u64; 2],
    /// `host_read_commands`.
    pub host_read_commands: [u64; 2],
    /// `host_write_commands`.
    pub host_write_commands: [u64; 2],
    /// `busy_time`.
    pub busy_time: [u64; 2],
    /// `power_cycles`.
    pub power_cycles: [u64; 2],
    /// `power_on_hours`.
    pub power_on_hours: [u64; 2],
    /// `unsafe_shutdowns`.
    pub unsafe_shutdowns: [u64; 2],
    /// `integrity_errors`.
    pub integrity_errors: [u64; 2],
    /// `error_log_entries`.
    pub error_log_entries: [u64; 2],
    /// `warn_temp_time`: 1.2+.
    pub warn_temp_time: u32,
    /// `crit_temp_time`: 1.2+.
    pub crit_temp_time: u32,
    /// `temp_sensors`: 1.2+.
    pub temp_sensors: [u16; 8],
    /// `therm_mgmt_count_1`: 1.3+.
    pub therm_mgmt_count_1: u32,
    /// `therm_mgmt_count_2`: 1.3+.
    pub therm_mgmt_count_2: u32,
    /// `therm_mgmt_time_1`: 1.3+.
    pub therm_mgmt_time_1: u32,
    /// `therm_mgmt_time_2`: 1.3+.
    pub therm_mgmt_time_2: u32,
    /// `_reserved2`.
    pub _reserved2: [u8; 280],
}

/// `NVM_HEALTH_CW_SPARE`.
pub const NVM_HEALTH_CW_SPARE: u8 = 1 << 0;
/// `NVM_HEALTH_CW_TEMP`.
pub const NVM_HEALTH_CW_TEMP: u8 = 1 << 1;
/// `NVM_HEALTH_CW_MEDIA`.
pub const NVM_HEALTH_CW_MEDIA: u8 = 1 << 2;
/// `NVM_HEALTH_CW_READONLY`.
pub const NVM_HEALTH_CW_READONLY: u8 = 1 << 3;
/// `NVM_HEALTH_CW_VOLATILE`.
pub const NVM_HEALTH_CW_VOLATILE: u8 = 1 << 4;
/// `NVM_HEALTH_CW_PMR`.
pub const NVM_HEALTH_CW_PMR: u8 = 1 << 5;

const _: () = {
    use core::mem::{align_of, offset_of, size_of};

    assert!(size_of::<NvmeSge>() == 16 && align_of::<NvmeSge>() == 8);
    assert!(size_of::<NvmeSgeData>() == 16 && size_of::<NvmeSgeBitBucket>() == 16);
    assert!(size_of::<NvmeSqeEntry>() == 16);
    assert!(size_of::<NvmeSqe>() == 64 && align_of::<NvmeSqe>() == 8);
    assert!(offset_of!(NvmeSqe, mptr) == 16 && offset_of!(NvmeSqe, entry) == 24);
    assert!(offset_of!(NvmeSqe, cdw10) == 40 && offset_of!(NvmeSqe, cdw15) == 60);
    assert!(size_of::<NvmeSqeQ>() == 64 && align_of::<NvmeSqeQ>() == 8);
    assert!(offset_of!(NvmeSqeQ, prp1) == 24 && offset_of!(NvmeSqeQ, qid) == 40);
    assert!(offset_of!(NvmeSqeQ, qflags) == 44 && offset_of!(NvmeSqeQ, cqid) == 46);
    assert!(size_of::<NvmeSqeIo>() == 64 && align_of::<NvmeSqeIo>() == 8);
    assert!(offset_of!(NvmeSqeIo, slba) == 40 && offset_of!(NvmeSqeIo, nlb) == 48);
    assert!(offset_of!(NvmeSqeIo, eilbrt) == 56 && offset_of!(NvmeSqeIo, elbatm) == 62);
    assert!(size_of::<NvmeCqe>() == 16);
    assert!(offset_of!(NvmeCqe, cid) == 12 && offset_of!(NvmeCqe, flags) == 14);
    assert!(size_of::<NvmIdentifyPsd>() == 32);
    assert!(size_of::<NvmIdentifyController>() == 4096);
    assert!(offset_of!(NvmIdentifyController, mn) == 24);
    assert!(offset_of!(NvmIdentifyController, fr) == 64);
    assert!(offset_of!(NvmIdentifyController, mdts) == 77);
    assert!(offset_of!(NvmIdentifyController, ctratt) == 96);
    assert!(offset_of!(NvmIdentifyController, oacs) == 256);
    assert!(offset_of!(NvmIdentifyController, sanicap) == 328);
    assert!(offset_of!(NvmIdentifyController, sqes) == 512);
    assert!(offset_of!(NvmIdentifyController, nn) == 516);
    assert!(offset_of!(NvmIdentifyController, sgls) == 536);
    assert!(offset_of!(NvmIdentifyController, psd) == 2048);
    assert!(size_of::<NvmNamespaceFormat>() == 4);
    assert!(size_of::<NvmIdentifyNamespace>() == 4096);
    assert!(offset_of!(NvmIdentifyNamespace, nsfeat) == 24);
    assert!(offset_of!(NvmIdentifyNamespace, nguid) == 104);
    assert!(offset_of!(NvmIdentifyNamespace, lbaf) == 128);
    assert!(offset_of!(NvmIdentifyNamespace, vs) == 384);
    assert!(size_of::<NvmSmartHealth>() == 512);
    assert!(offset_of!(NvmSmartHealth, temperature) == 1);
    assert!(offset_of!(NvmSmartHealth, data_units_read) == 32);
    assert!(offset_of!(NvmSmartHealth, warn_temp_time) == 192);
    assert!(offset_of!(NvmSmartHealth, temp_sensors) == 200);
};

#[cfg(test)]
mod tests;
