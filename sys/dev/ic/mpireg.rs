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
 * Copyright (c) 2005 David Gwynne <dlg@openbsd.org>
 * Copyright (c) 2005 Marco Peereboom <marco@openbsd.org>
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
//! `<dev/ic/mpireg.h>`: the LSI Fusion-MPT (MPI 1.5) register set, scatter/gather elements,
//! message frames (IOC init and facts, port facts and enable, events, SCSI I/O and task
//! management, RAID actions, configuration requests) and configuration pages (SCSI SPI,
//! Fibre Channel, SAS, IOC and RAID).
//!
//! Upstream: sys/dev/ic/mpireg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Every constant and structure of the header, in its order. The function-like macros
//!   (`MPI_DOORBELL_FUNCTION(x)`, `MPI_DOORBELL_DWORDS(x)`, `MPI_EVT_SASPHY_LINK_CUR(x)`,
//!   `MPI_EVT_SASPHY_LINK_PREV(x)`, `MPI_CFG_SPI_DEV_2_DATA_PIPE_SELECT(x)`) are lower-case
//!   `const fn`s (`docs/C_TO_RUST.md`).
//! - Types of the constants: register offsets are `usize` (`bus_size_t`), `MPI_FUNCTION_*`,
//!   `MPI_WHOINIT_*` and `MPI_REP_FLAGS_CONT` `u8`, `MPI_IOCSTATUS_*` and `MPI_REP_IOCSTATUS*`
//!   `u16`, the flags defined inside a structure the type of the member they follow, the rest
//!   `u32`; `MPI_CDB_LEN` is a `usize` (an array length).
//! - The structures are `__packed __aligned(4)` in C. Where every member sits at its natural
//!   alignment and the size is a multiple of four they are plain `#[repr(C)]`; where a member
//!   does not (a `u16` at an odd place, a `u64` at a multiple of four) they are
//!   `#[repr(C, packed)]`, whose fields are only read and written by value, never borrowed.
//!   Rust does not combine `packed` with `align`, so the C's four-byte alignment of the
//!   structures made only of bytes is not stated; they sit in DMA frames that are aligned.
//!   Sizes and member offsets are asserted at compile time. `struct
//!   mpi_msg_eventack_reply` is 22 bytes packed (its `ioc_status` is a `u32` at offset 14,
//!   which looks like a typo for `u16` upstream); the C pads it to 24 and nothing uses its
//!   size.
//! - The anonymous packed `device_settings[16]` element of `struct mpi_cfg_spi_port_pg2` is
//!   [`MpiCfgSpiPortPg2DeviceSettings`].
//! - `char` members (`chip_name`, ...) are `u8` arrays.
//! - The `#if notyet` block of `MPI_SCSIIO_ERR_STATUS_*` (macros with no value) is not ported.

/// `MPI_DOORBELL`.
pub const MPI_DOORBELL: usize = 0x00;
/// `MPI_DOORBELL_STATE`: ioc state.
pub const MPI_DOORBELL_STATE: u32 = 0xf << 28;
/// `MPI_DOORBELL_STATE_RESET`.
pub const MPI_DOORBELL_STATE_RESET: u32 = 0;
/// `MPI_DOORBELL_STATE_READY`.
pub const MPI_DOORBELL_STATE_READY: u32 = 0x1 << 28;
/// `MPI_DOORBELL_STATE_OPER`.
pub const MPI_DOORBELL_STATE_OPER: u32 = 0x2 << 28;
/// `MPI_DOORBELL_STATE_FAULT`.
pub const MPI_DOORBELL_STATE_FAULT: u32 = 0x4 << 28;
/// `MPI_DOORBELL_INUSE`: doorbell used.
pub const MPI_DOORBELL_INUSE: u32 = 0x1 << 27;
/// `MPI_DOORBELL_WHOINIT`: last to reset ioc.
pub const MPI_DOORBELL_WHOINIT: u32 = 0x7 << 24;
/// `MPI_DOORBELL_WHOINIT_NOONE`: not initialized.
pub const MPI_DOORBELL_WHOINIT_NOONE: u32 = 0;
/// `MPI_DOORBELL_WHOINIT_SYSBIOS`: system bios.
pub const MPI_DOORBELL_WHOINIT_SYSBIOS: u32 = 0x1 << 24;
/// `MPI_DOORBELL_WHOINIT_ROMBIOS`: rom bios.
pub const MPI_DOORBELL_WHOINIT_ROMBIOS: u32 = 0x2 << 24;
/// `MPI_DOORBELL_WHOINIT_PCIPEER`: pci peer.
pub const MPI_DOORBELL_WHOINIT_PCIPEER: u32 = 0x3 << 24;
/// `MPI_DOORBELL_WHOINIT_DRIVER`: host driver.
pub const MPI_DOORBELL_WHOINIT_DRIVER: u32 = 0x4 << 24;
/// `MPI_DOORBELL_WHOINIT_MANUFACT`: manufacturing.
pub const MPI_DOORBELL_WHOINIT_MANUFACT: u32 = 0x5 << 24;
/// `MPI_DOORBELL_FAULT`: fault code.
pub const MPI_DOORBELL_FAULT: u32 = 0xffff;
/// `MPI_DOORBELL_FAULT_REQ_PCIPAR`: req msg pci parity err.
pub const MPI_DOORBELL_FAULT_REQ_PCIPAR: u32 = 0x8111;
/// `MPI_DOORBELL_FAULT_REQ_PCIBUS`: req msg pci bus err.
pub const MPI_DOORBELL_FAULT_REQ_PCIBUS: u32 = 0x8112;
/// `MPI_DOORBELL_FAULT_REP_PCIPAR`: reply msg pci parity err.
pub const MPI_DOORBELL_FAULT_REP_PCIPAR: u32 = 0x8113;
/// `MPI_DOORBELL_FAULT_REP_PCIBUS`: reply msg pci bus err.
pub const MPI_DOORBELL_FAULT_REP_PCIBUS: u32 = 0x8114;
/// `MPI_DOORBELL_FAULT_SND_PCIPAR`: data send pci parity err.
pub const MPI_DOORBELL_FAULT_SND_PCIPAR: u32 = 0x8115;
/// `MPI_DOORBELL_FAULT_SND_PCIBUS`: data send pci bus err.
pub const MPI_DOORBELL_FAULT_SND_PCIBUS: u32 = 0x8116;
/// `MPI_DOORBELL_FAULT_RCV_PCIPAR`: data recv pci parity err.
pub const MPI_DOORBELL_FAULT_RCV_PCIPAR: u32 = 0x8117;
/// `MPI_DOORBELL_FAULT_RCV_PCIBUS`: data recv pci bus err.
pub const MPI_DOORBELL_FAULT_RCV_PCIBUS: u32 = 0x8118;
/// `MPI_DOORBELL_FUNCTION_SHIFT`.
pub const MPI_DOORBELL_FUNCTION_SHIFT: u32 = 24;
/// `MPI_DOORBELL_FUNCTION_MASK`.
pub const MPI_DOORBELL_FUNCTION_MASK: u32 = 0xff << MPI_DOORBELL_FUNCTION_SHIFT;
/// `MPI_DOORBELL_FUNCTION(x)`.
pub const fn mpi_doorbell_function(x: u32) -> u32 {
    (x << MPI_DOORBELL_FUNCTION_SHIFT) & MPI_DOORBELL_FUNCTION_MASK
}
/// `MPI_DOORBELL_DWORDS_SHIFT`.
pub const MPI_DOORBELL_DWORDS_SHIFT: u32 = 16;
/// `MPI_DOORBELL_DWORDS_MASK`.
pub const MPI_DOORBELL_DWORDS_MASK: u32 = 0xff << MPI_DOORBELL_DWORDS_SHIFT;
/// `MPI_DOORBELL_DWORDS(x)`.
pub const fn mpi_doorbell_dwords(x: u32) -> u32 {
    (x << MPI_DOORBELL_DWORDS_SHIFT) & MPI_DOORBELL_DWORDS_MASK
}
/// `MPI_DOORBELL_DATA_MASK`.
pub const MPI_DOORBELL_DATA_MASK: u32 = 0xffff;
/// `MPI_WRITESEQ`.
pub const MPI_WRITESEQ: usize = 0x04;
/// `MPI_WRITESEQ_VALUE`: key value.
pub const MPI_WRITESEQ_VALUE: u32 = 0x0000000f;
/// `MPI_WRITESEQ_1`.
pub const MPI_WRITESEQ_1: u32 = 0x04;
/// `MPI_WRITESEQ_2`.
pub const MPI_WRITESEQ_2: u32 = 0x0b;
/// `MPI_WRITESEQ_3`.
pub const MPI_WRITESEQ_3: u32 = 0x02;
/// `MPI_WRITESEQ_4`.
pub const MPI_WRITESEQ_4: u32 = 0x07;
/// `MPI_WRITESEQ_5`.
pub const MPI_WRITESEQ_5: u32 = 0x0d;
/// `MPI_HOSTDIAG`.
pub const MPI_HOSTDIAG: usize = 0x08;
/// `MPI_HOSTDIAG_CLEARFBS`: clear flash bad sig.
pub const MPI_HOSTDIAG_CLEARFBS: u32 = 1 << 10;
/// `MPI_HOSTDIAG_POICB`: prevent ioc boot.
pub const MPI_HOSTDIAG_POICB: u32 = 1 << 9;
/// `MPI_HOSTDIAG_DWRE`: diag reg write enabled.
pub const MPI_HOSTDIAG_DWRE: u32 = 1 << 7;
/// `MPI_HOSTDIAG_FBS`: flash bad sig.
pub const MPI_HOSTDIAG_FBS: u32 = 1 << 6;
/// `MPI_HOSTDIAG_RESET_HIST`: reset history.
pub const MPI_HOSTDIAG_RESET_HIST: u32 = 1 << 5;
/// `MPI_HOSTDIAG_DIAGWR_EN`: diagnostic write enabled.
pub const MPI_HOSTDIAG_DIAGWR_EN: u32 = 1 << 4;
/// `MPI_HOSTDIAG_RESET_ADAPTER`: reset adapter.
pub const MPI_HOSTDIAG_RESET_ADAPTER: u32 = 1 << 2;
/// `MPI_HOSTDIAG_DISABLE_ARM`: disable arm.
pub const MPI_HOSTDIAG_DISABLE_ARM: u32 = 1 << 1;
/// `MPI_HOSTDIAG_DIAGMEM_EN`: diag mem enable.
pub const MPI_HOSTDIAG_DIAGMEM_EN: u32 = 1;
/// `MPI_TESTBASE`.
pub const MPI_TESTBASE: usize = 0x0c;
/// `MPI_DIAGRWDATA`.
pub const MPI_DIAGRWDATA: usize = 0x10;
/// `MPI_DIAGRWADDR`.
pub const MPI_DIAGRWADDR: usize = 0x18;
/// `MPI_INTR_STATUS`.
pub const MPI_INTR_STATUS: usize = 0x30;
/// `MPI_INTR_STATUS_IOCDOORBELL`: ioc doorbell status.
pub const MPI_INTR_STATUS_IOCDOORBELL: u32 = 1 << 31;
/// `MPI_INTR_STATUS_REPLY`: reply message interrupt.
pub const MPI_INTR_STATUS_REPLY: u32 = 1 << 3;
/// `MPI_INTR_STATUS_DOORBELL`: doorbell interrupt.
pub const MPI_INTR_STATUS_DOORBELL: u32 = 1;
/// `MPI_INTR_MASK`.
pub const MPI_INTR_MASK: usize = 0x34;
/// `MPI_INTR_MASK_REPLY`: reply message intr mask.
pub const MPI_INTR_MASK_REPLY: u32 = 1 << 3;
/// `MPI_INTR_MASK_DOORBELL`: doorbell interrupt mask.
pub const MPI_INTR_MASK_DOORBELL: u32 = 1;
/// `MPI_REQ_QUEUE`.
pub const MPI_REQ_QUEUE: usize = 0x40;
/// `MPI_REPLY_QUEUE`.
pub const MPI_REPLY_QUEUE: usize = 0x44;
/// `MPI_REPLY_QUEUE_ADDRESS`: address reply.
pub const MPI_REPLY_QUEUE_ADDRESS: u32 = 1 << 31;
/// `MPI_REPLY_QUEUE_ADDRESS_MASK`.
pub const MPI_REPLY_QUEUE_ADDRESS_MASK: u32 = 0x7fffffff;
/// `MPI_REPLY_QUEUE_TYPE_MASK`.
pub const MPI_REPLY_QUEUE_TYPE_MASK: u32 = 3 << 29;
/// `MPI_REPLY_QUEUE_TYPE_INIT`: scsi initiator reply.
pub const MPI_REPLY_QUEUE_TYPE_INIT: u32 = 0;
/// `MPI_REPLY_QUEUE_TYPE_TARGET`: scsi target reply.
pub const MPI_REPLY_QUEUE_TYPE_TARGET: u32 = 1 << 29;
/// `MPI_REPLY_QUEUE_TYPE_LAN`: lan reply.
pub const MPI_REPLY_QUEUE_TYPE_LAN: u32 = 2 << 29;
/// `MPI_REPLY_QUEUE_CONTEXT`: not address and type.
pub const MPI_REPLY_QUEUE_CONTEXT: u32 = 0x1fffffff;
/// `MPI_PRIREQ_QUEUE`.
pub const MPI_PRIREQ_QUEUE: usize = 0x48;
/// `MPI_SGE_FL_LAST`: last element in segment.
pub const MPI_SGE_FL_LAST: u32 = 0x1 << 31;
/// `MPI_SGE_FL_EOB`: last element of buffer.
pub const MPI_SGE_FL_EOB: u32 = 0x1 << 30;
/// `MPI_SGE_FL_TYPE`: element type.
pub const MPI_SGE_FL_TYPE: u32 = 0x3 << 28;
/// `MPI_SGE_FL_TYPE_SIMPLE`: simple element.
pub const MPI_SGE_FL_TYPE_SIMPLE: u32 = 0x1 << 28;
/// `MPI_SGE_FL_TYPE_CHAIN`: chain element.
pub const MPI_SGE_FL_TYPE_CHAIN: u32 = 0x3 << 28;
/// `MPI_SGE_FL_TYPE_XACTCTX`: transaction context.
pub const MPI_SGE_FL_TYPE_XACTCTX: u32 = 0;
/// `MPI_SGE_FL_LOCAL`: local address.
pub const MPI_SGE_FL_LOCAL: u32 = 0x1 << 27;
/// `MPI_SGE_FL_DIR`: direction.
pub const MPI_SGE_FL_DIR: u32 = 0x1 << 26;
/// `MPI_SGE_FL_DIR_OUT`.
pub const MPI_SGE_FL_DIR_OUT: u32 = 0x1 << 26;
/// `MPI_SGE_FL_DIR_IN`.
pub const MPI_SGE_FL_DIR_IN: u32 = 0;
/// `MPI_SGE_FL_SIZE`: address size.
pub const MPI_SGE_FL_SIZE: u32 = 0x1 << 25;
/// `MPI_SGE_FL_SIZE_32`.
pub const MPI_SGE_FL_SIZE_32: u32 = 0;
/// `MPI_SGE_FL_SIZE_64`.
pub const MPI_SGE_FL_SIZE_64: u32 = 0x1 << 25;
/// `MPI_SGE_FL_EOL`: end of list.
pub const MPI_SGE_FL_EOL: u32 = 0x1 << 24;
/// `MPI_SGE_FLAGS_IOC_TO_HOST`.
pub const MPI_SGE_FLAGS_IOC_TO_HOST: u32 = 0x00;
/// `MPI_SGE_FLAGS_HOST_TO_IOC`.
pub const MPI_SGE_FLAGS_HOST_TO_IOC: u32 = 0x04;
/// `struct mpi_sge`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiSge {
    /// `sg_hdr`.
    pub sg_hdr: u32,
    /// `sg_addr_lo`.
    pub sg_addr_lo: u32,
    /// `sg_addr_hi`.
    pub sg_addr_hi: u32,
}
const _: () = {
    assert!(size_of::<MpiSge>() == 12);
    assert!(core::mem::offset_of!(MpiSge, sg_hdr) == 0);
    assert!(core::mem::offset_of!(MpiSge, sg_addr_lo) == 4);
    assert!(core::mem::offset_of!(MpiSge, sg_addr_hi) == 8);
};
/// `struct mpi_fw_tce`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiFwTce {
    /// `reserved1`.
    pub reserved1: u8,
    /// `context_size`.
    pub context_size: u8,
    /// `details_length`.
    pub details_length: u8,
    /// `flags`.
    pub flags: u8,
    /// `reserved2`.
    pub reserved2: u32,
    /// `image_offset`.
    pub image_offset: u32,
    /// `image_size`.
    pub image_size: u32,
}
const _: () = {
    assert!(size_of::<MpiFwTce>() == 16);
    assert!(core::mem::offset_of!(MpiFwTce, context_size) == 1);
    assert!(core::mem::offset_of!(MpiFwTce, details_length) == 2);
    assert!(core::mem::offset_of!(MpiFwTce, flags) == 3);
    assert!(core::mem::offset_of!(MpiFwTce, image_offset) == 8);
    assert!(core::mem::offset_of!(MpiFwTce, image_size) == 12);
};
/// `MPI_FUNCTION_SCSI_IO_REQUEST`.
pub const MPI_FUNCTION_SCSI_IO_REQUEST: u8 = 0x00;
/// `MPI_FUNCTION_SCSI_TASK_MGMT`.
pub const MPI_FUNCTION_SCSI_TASK_MGMT: u8 = 0x01;
/// `MPI_FUNCTION_IOC_INIT`.
pub const MPI_FUNCTION_IOC_INIT: u8 = 0x02;
/// `MPI_FUNCTION_IOC_FACTS`.
pub const MPI_FUNCTION_IOC_FACTS: u8 = 0x03;
/// `MPI_FUNCTION_CONFIG`.
pub const MPI_FUNCTION_CONFIG: u8 = 0x04;
/// `MPI_FUNCTION_PORT_FACTS`.
pub const MPI_FUNCTION_PORT_FACTS: u8 = 0x05;
/// `MPI_FUNCTION_PORT_ENABLE`.
pub const MPI_FUNCTION_PORT_ENABLE: u8 = 0x06;
/// `MPI_FUNCTION_EVENT_NOTIFICATION`.
pub const MPI_FUNCTION_EVENT_NOTIFICATION: u8 = 0x07;
/// `MPI_FUNCTION_EVENT_ACK`.
pub const MPI_FUNCTION_EVENT_ACK: u8 = 0x08;
/// `MPI_FUNCTION_FW_DOWNLOAD`.
pub const MPI_FUNCTION_FW_DOWNLOAD: u8 = 0x09;
/// `MPI_FUNCTION_TARGET_CMD_BUFFER_POST`.
pub const MPI_FUNCTION_TARGET_CMD_BUFFER_POST: u8 = 0x0A;
/// `MPI_FUNCTION_TARGET_ASSIST`.
pub const MPI_FUNCTION_TARGET_ASSIST: u8 = 0x0B;
/// `MPI_FUNCTION_TARGET_STATUS_SEND`.
pub const MPI_FUNCTION_TARGET_STATUS_SEND: u8 = 0x0C;
/// `MPI_FUNCTION_TARGET_MODE_ABORT`.
pub const MPI_FUNCTION_TARGET_MODE_ABORT: u8 = 0x0D;
/// `MPI_FUNCTION_TARGET_FC_BUF_POST_LINK_SRVC`: obsolete.
pub const MPI_FUNCTION_TARGET_FC_BUF_POST_LINK_SRVC: u8 = 0x0E;
/// `MPI_FUNCTION_TARGET_FC_RSP_LINK_SRVC`: obsolete.
pub const MPI_FUNCTION_TARGET_FC_RSP_LINK_SRVC: u8 = 0x0F;
/// `MPI_FUNCTION_TARGET_FC_EX_SEND_LINK_SRVC`: obsolete.
pub const MPI_FUNCTION_TARGET_FC_EX_SEND_LINK_SRVC: u8 = 0x10;
/// `MPI_FUNCTION_TARGET_FC_ABORT`: obsolete.
pub const MPI_FUNCTION_TARGET_FC_ABORT: u8 = 0x11;
/// `MPI_FUNCTION_FC_LINK_SRVC_BUF_POST`.
pub const MPI_FUNCTION_FC_LINK_SRVC_BUF_POST: u8 = 0x0E;
/// `MPI_FUNCTION_FC_LINK_SRVC_RSP`.
pub const MPI_FUNCTION_FC_LINK_SRVC_RSP: u8 = 0x0F;
/// `MPI_FUNCTION_FC_EX_LINK_SRVC_SEND`.
pub const MPI_FUNCTION_FC_EX_LINK_SRVC_SEND: u8 = 0x10;
/// `MPI_FUNCTION_FC_ABORT`.
pub const MPI_FUNCTION_FC_ABORT: u8 = 0x11;
/// `MPI_FUNCTION_FW_UPLOAD`.
pub const MPI_FUNCTION_FW_UPLOAD: u8 = 0x12;
/// `MPI_FUNCTION_FC_COMMON_TRANSPORT_SEND`.
pub const MPI_FUNCTION_FC_COMMON_TRANSPORT_SEND: u8 = 0x13;
/// `MPI_FUNCTION_FC_PRIMITIVE_SEND`.
pub const MPI_FUNCTION_FC_PRIMITIVE_SEND: u8 = 0x14;
/// `MPI_FUNCTION_RAID_ACTION`.
pub const MPI_FUNCTION_RAID_ACTION: u8 = 0x15;
/// `MPI_FUNCTION_RAID_SCSI_IO_PASSTHROUGH`.
pub const MPI_FUNCTION_RAID_SCSI_IO_PASSTHROUGH: u8 = 0x16;
/// `MPI_FUNCTION_TOOLBOX`.
pub const MPI_FUNCTION_TOOLBOX: u8 = 0x17;
/// `MPI_FUNCTION_SCSI_ENCLOSURE_PROCESSOR`.
pub const MPI_FUNCTION_SCSI_ENCLOSURE_PROCESSOR: u8 = 0x18;
/// `MPI_FUNCTION_MAILBOX`.
pub const MPI_FUNCTION_MAILBOX: u8 = 0x19;
/// `MPI_FUNCTION_LAN_SEND`.
pub const MPI_FUNCTION_LAN_SEND: u8 = 0x20;
/// `MPI_FUNCTION_LAN_RECEIVE`.
pub const MPI_FUNCTION_LAN_RECEIVE: u8 = 0x21;
/// `MPI_FUNCTION_LAN_RESET`.
pub const MPI_FUNCTION_LAN_RESET: u8 = 0x22;
/// `MPI_FUNCTION_IOC_MESSAGE_UNIT_RESET`.
pub const MPI_FUNCTION_IOC_MESSAGE_UNIT_RESET: u8 = 0x40;
/// `MPI_FUNCTION_IO_UNIT_RESET`.
pub const MPI_FUNCTION_IO_UNIT_RESET: u8 = 0x41;
/// `MPI_FUNCTION_HANDSHAKE`.
pub const MPI_FUNCTION_HANDSHAKE: u8 = 0x42;
/// `MPI_FUNCTION_REPLY_FRAME_REMOVAL`.
pub const MPI_FUNCTION_REPLY_FRAME_REMOVAL: u8 = 0x43;
/// `MPI_REP_FLAGS_CONT`: continuation reply.
pub const MPI_REP_FLAGS_CONT: u8 = 1 << 7;
/// `MPI_REP_IOCSTATUS_AVAIL`: logging info available.
pub const MPI_REP_IOCSTATUS_AVAIL: u16 = 1 << 15;
/// `MPI_REP_IOCSTATUS`: status.
pub const MPI_REP_IOCSTATUS: u16 = 0x7fff;
/// `MPI_IOCSTATUS_SUCCESS`.
pub const MPI_IOCSTATUS_SUCCESS: u16 = 0x0000;
/// `MPI_IOCSTATUS_INVALID_FUNCTION`.
pub const MPI_IOCSTATUS_INVALID_FUNCTION: u16 = 0x0001;
/// `MPI_IOCSTATUS_BUSY`.
pub const MPI_IOCSTATUS_BUSY: u16 = 0x0002;
/// `MPI_IOCSTATUS_INVALID_SGL`.
pub const MPI_IOCSTATUS_INVALID_SGL: u16 = 0x0003;
/// `MPI_IOCSTATUS_INTERNAL_ERROR`.
pub const MPI_IOCSTATUS_INTERNAL_ERROR: u16 = 0x0004;
/// `MPI_IOCSTATUS_RESERVED`.
pub const MPI_IOCSTATUS_RESERVED: u16 = 0x0005;
/// `MPI_IOCSTATUS_INSUFFICIENT_RESOURCES`.
pub const MPI_IOCSTATUS_INSUFFICIENT_RESOURCES: u16 = 0x0006;
/// `MPI_IOCSTATUS_INVALID_FIELD`.
pub const MPI_IOCSTATUS_INVALID_FIELD: u16 = 0x0007;
/// `MPI_IOCSTATUS_INVALID_STATE`.
pub const MPI_IOCSTATUS_INVALID_STATE: u16 = 0x0008;
/// `MPI_IOCSTATUS_OP_STATE_NOT_SUPPORTED`.
pub const MPI_IOCSTATUS_OP_STATE_NOT_SUPPORTED: u16 = 0x0009;
/// `MPI_IOCSTATUS_CONFIG_INVALID_ACTION`.
pub const MPI_IOCSTATUS_CONFIG_INVALID_ACTION: u16 = 0x0020;
/// `MPI_IOCSTATUS_CONFIG_INVALID_TYPE`.
pub const MPI_IOCSTATUS_CONFIG_INVALID_TYPE: u16 = 0x0021;
/// `MPI_IOCSTATUS_CONFIG_INVALID_PAGE`.
pub const MPI_IOCSTATUS_CONFIG_INVALID_PAGE: u16 = 0x0022;
/// `MPI_IOCSTATUS_CONFIG_INVALID_DATA`.
pub const MPI_IOCSTATUS_CONFIG_INVALID_DATA: u16 = 0x0023;
/// `MPI_IOCSTATUS_CONFIG_NO_DEFAULTS`.
pub const MPI_IOCSTATUS_CONFIG_NO_DEFAULTS: u16 = 0x0024;
/// `MPI_IOCSTATUS_CONFIG_CANT_COMMIT`.
pub const MPI_IOCSTATUS_CONFIG_CANT_COMMIT: u16 = 0x0025;
/// `MPI_IOCSTATUS_SCSI_RECOVERED_ERROR`.
pub const MPI_IOCSTATUS_SCSI_RECOVERED_ERROR: u16 = 0x0040;
/// `MPI_IOCSTATUS_SCSI_INVALID_BUS`.
pub const MPI_IOCSTATUS_SCSI_INVALID_BUS: u16 = 0x0041;
/// `MPI_IOCSTATUS_SCSI_INVALID_TARGETID`.
pub const MPI_IOCSTATUS_SCSI_INVALID_TARGETID: u16 = 0x0042;
/// `MPI_IOCSTATUS_SCSI_DEVICE_NOT_THERE`.
pub const MPI_IOCSTATUS_SCSI_DEVICE_NOT_THERE: u16 = 0x0043;
/// `MPI_IOCSTATUS_SCSI_DATA_OVERRUN`.
pub const MPI_IOCSTATUS_SCSI_DATA_OVERRUN: u16 = 0x0044;
/// `MPI_IOCSTATUS_SCSI_DATA_UNDERRUN`.
pub const MPI_IOCSTATUS_SCSI_DATA_UNDERRUN: u16 = 0x0045;
/// `MPI_IOCSTATUS_SCSI_IO_DATA_ERROR`.
pub const MPI_IOCSTATUS_SCSI_IO_DATA_ERROR: u16 = 0x0046;
/// `MPI_IOCSTATUS_SCSI_PROTOCOL_ERROR`.
pub const MPI_IOCSTATUS_SCSI_PROTOCOL_ERROR: u16 = 0x0047;
/// `MPI_IOCSTATUS_SCSI_TASK_TERMINATED`.
pub const MPI_IOCSTATUS_SCSI_TASK_TERMINATED: u16 = 0x0048;
/// `MPI_IOCSTATUS_SCSI_RESIDUAL_MISMATCH`.
pub const MPI_IOCSTATUS_SCSI_RESIDUAL_MISMATCH: u16 = 0x0049;
/// `MPI_IOCSTATUS_SCSI_TASK_MGMT_FAILED`.
pub const MPI_IOCSTATUS_SCSI_TASK_MGMT_FAILED: u16 = 0x004A;
/// `MPI_IOCSTATUS_SCSI_IOC_TERMINATED`.
pub const MPI_IOCSTATUS_SCSI_IOC_TERMINATED: u16 = 0x004B;
/// `MPI_IOCSTATUS_SCSI_EXT_TERMINATED`.
pub const MPI_IOCSTATUS_SCSI_EXT_TERMINATED: u16 = 0x004C;
/// `MPI_IOCSTATUS_EEDP_GUARD_ERROR`.
pub const MPI_IOCSTATUS_EEDP_GUARD_ERROR: u16 = 0x004D;
/// `MPI_IOCSTATUS_EEDP_REF_TAG_ERROR`.
pub const MPI_IOCSTATUS_EEDP_REF_TAG_ERROR: u16 = 0x004E;
/// `MPI_IOCSTATUS_EEDP_APP_TAG_ERROR`.
pub const MPI_IOCSTATUS_EEDP_APP_TAG_ERROR: u16 = 0x004F;
/// `MPI_IOCSTATUS_TARGET_PRIORITY_IO`.
pub const MPI_IOCSTATUS_TARGET_PRIORITY_IO: u16 = 0x0060;
/// `MPI_IOCSTATUS_TARGET_INVALID_PORT`.
pub const MPI_IOCSTATUS_TARGET_INVALID_PORT: u16 = 0x0061;
/// `MPI_IOCSTATUS_TARGET_INVALID_IOCINDEX`: obsolete.
pub const MPI_IOCSTATUS_TARGET_INVALID_IOCINDEX: u16 = 0x0062;
/// `MPI_IOCSTATUS_TARGET_INVALID_IO_INDEX`.
pub const MPI_IOCSTATUS_TARGET_INVALID_IO_INDEX: u16 = 0x0062;
/// `MPI_IOCSTATUS_TARGET_ABORTED`.
pub const MPI_IOCSTATUS_TARGET_ABORTED: u16 = 0x0063;
/// `MPI_IOCSTATUS_TARGET_NO_CONN_RETRYABLE`.
pub const MPI_IOCSTATUS_TARGET_NO_CONN_RETRYABLE: u16 = 0x0064;
/// `MPI_IOCSTATUS_TARGET_NO_CONNECTION`.
pub const MPI_IOCSTATUS_TARGET_NO_CONNECTION: u16 = 0x0065;
/// `MPI_IOCSTATUS_TARGET_XFER_COUNT_MISMATCH`.
pub const MPI_IOCSTATUS_TARGET_XFER_COUNT_MISMATCH: u16 = 0x006A;
/// `MPI_IOCSTATUS_TARGET_STS_DATA_NOT_SENT`.
pub const MPI_IOCSTATUS_TARGET_STS_DATA_NOT_SENT: u16 = 0x006B;
/// `MPI_IOCSTATUS_TARGET_DATA_OFFSET_ERROR`.
pub const MPI_IOCSTATUS_TARGET_DATA_OFFSET_ERROR: u16 = 0x006D;
/// `MPI_IOCSTATUS_TARGET_TOO_MUCH_WRITE_DATA`.
pub const MPI_IOCSTATUS_TARGET_TOO_MUCH_WRITE_DATA: u16 = 0x006E;
/// `MPI_IOCSTATUS_TARGET_IU_TOO_SHORT`.
pub const MPI_IOCSTATUS_TARGET_IU_TOO_SHORT: u16 = 0x006F;
/// `MPI_IOCSTATUS_TARGET_FC_ABORTED`: obsolete.
pub const MPI_IOCSTATUS_TARGET_FC_ABORTED: u16 = 0x0066;
/// `MPI_IOCSTATUS_TARGET_FC_RX_ID_INVALID`: obsolete.
pub const MPI_IOCSTATUS_TARGET_FC_RX_ID_INVALID: u16 = 0x0067;
/// `MPI_IOCSTATUS_TARGET_FC_DID_INVALID`: obsolete.
pub const MPI_IOCSTATUS_TARGET_FC_DID_INVALID: u16 = 0x0068;
/// `MPI_IOCSTATUS_TARGET_FC_NODE_LOGGED_OUT`: obsolete.
pub const MPI_IOCSTATUS_TARGET_FC_NODE_LOGGED_OUT: u16 = 0x0069;
/// `MPI_IOCSTATUS_FC_ABORTED`.
pub const MPI_IOCSTATUS_FC_ABORTED: u16 = 0x0066;
/// `MPI_IOCSTATUS_FC_RX_ID_INVALID`.
pub const MPI_IOCSTATUS_FC_RX_ID_INVALID: u16 = 0x0067;
/// `MPI_IOCSTATUS_FC_DID_INVALID`.
pub const MPI_IOCSTATUS_FC_DID_INVALID: u16 = 0x0068;
/// `MPI_IOCSTATUS_FC_NODE_LOGGED_OUT`.
pub const MPI_IOCSTATUS_FC_NODE_LOGGED_OUT: u16 = 0x0069;
/// `MPI_IOCSTATUS_FC_EXCHANGE_CANCELED`.
pub const MPI_IOCSTATUS_FC_EXCHANGE_CANCELED: u16 = 0x006C;
/// `MPI_IOCSTATUS_LAN_DEVICE_NOT_FOUND`.
pub const MPI_IOCSTATUS_LAN_DEVICE_NOT_FOUND: u16 = 0x0080;
/// `MPI_IOCSTATUS_LAN_DEVICE_FAILURE`.
pub const MPI_IOCSTATUS_LAN_DEVICE_FAILURE: u16 = 0x0081;
/// `MPI_IOCSTATUS_LAN_TRANSMIT_ERROR`.
pub const MPI_IOCSTATUS_LAN_TRANSMIT_ERROR: u16 = 0x0082;
/// `MPI_IOCSTATUS_LAN_TRANSMIT_ABORTED`.
pub const MPI_IOCSTATUS_LAN_TRANSMIT_ABORTED: u16 = 0x0083;
/// `MPI_IOCSTATUS_LAN_RECEIVE_ERROR`.
pub const MPI_IOCSTATUS_LAN_RECEIVE_ERROR: u16 = 0x0084;
/// `MPI_IOCSTATUS_LAN_RECEIVE_ABORTED`.
pub const MPI_IOCSTATUS_LAN_RECEIVE_ABORTED: u16 = 0x0085;
/// `MPI_IOCSTATUS_LAN_PARTIAL_PACKET`.
pub const MPI_IOCSTATUS_LAN_PARTIAL_PACKET: u16 = 0x0086;
/// `MPI_IOCSTATUS_LAN_CANCELED`.
pub const MPI_IOCSTATUS_LAN_CANCELED: u16 = 0x0087;
/// `MPI_IOCSTATUS_SAS_SMP_REQUEST_FAILED`.
pub const MPI_IOCSTATUS_SAS_SMP_REQUEST_FAILED: u16 = 0x0090;
/// `MPI_IOCSTATUS_SAS_SMP_DATA_OVERRUN`.
pub const MPI_IOCSTATUS_SAS_SMP_DATA_OVERRUN: u16 = 0x0091;
/// `MPI_IOCSTATUS_INBAND_ABORTED`.
pub const MPI_IOCSTATUS_INBAND_ABORTED: u16 = 0x0098;
/// `MPI_IOCSTATUS_INBAND_NO_CONNECTION`.
pub const MPI_IOCSTATUS_INBAND_NO_CONNECTION: u16 = 0x0099;
/// `MPI_IOCSTATUS_DIAGNOSTIC_RELEASED`.
pub const MPI_IOCSTATUS_DIAGNOSTIC_RELEASED: u16 = 0x00A0;
/// `MPI_REP_IOCLOGINFO_TYPE`: logging info type.
pub const MPI_REP_IOCLOGINFO_TYPE: u32 = 0xf << 28;
/// `MPI_REP_IOCLOGINFO_TYPE_NONE`.
pub const MPI_REP_IOCLOGINFO_TYPE_NONE: u32 = 0;
/// `MPI_REP_IOCLOGINFO_TYPE_SCSI`.
pub const MPI_REP_IOCLOGINFO_TYPE_SCSI: u32 = 0x1 << 28;
/// `MPI_REP_IOCLOGINFO_TYPE_FC`.
pub const MPI_REP_IOCLOGINFO_TYPE_FC: u32 = 0x2 << 28;
/// `MPI_REP_IOCLOGINFO_TYPE_SAS`.
pub const MPI_REP_IOCLOGINFO_TYPE_SAS: u32 = 0x3 << 28;
/// `MPI_REP_IOCLOGINFO_TYPE_ISCSI`.
pub const MPI_REP_IOCLOGINFO_TYPE_ISCSI: u32 = 0x4 << 28;
/// `MPI_REP_IOCLOGINFO_DATA`: logging info data.
pub const MPI_REP_IOCLOGINFO_DATA: u32 = 0x0fffffff;
/// `MPI_EVENT_NONE`.
pub const MPI_EVENT_NONE: u32 = 0x00;
/// `MPI_EVENT_LOG_DATA`.
pub const MPI_EVENT_LOG_DATA: u32 = 0x01;
/// `MPI_EVENT_STATE_CHANGE`.
pub const MPI_EVENT_STATE_CHANGE: u32 = 0x02;
/// `MPI_EVENT_UNIT_ATTENTION`.
pub const MPI_EVENT_UNIT_ATTENTION: u32 = 0x03;
/// `MPI_EVENT_IOC_BUS_RESET`.
pub const MPI_EVENT_IOC_BUS_RESET: u32 = 0x04;
/// `MPI_EVENT_EXT_BUS_RESET`.
pub const MPI_EVENT_EXT_BUS_RESET: u32 = 0x05;
/// `MPI_EVENT_RESCAN`.
pub const MPI_EVENT_RESCAN: u32 = 0x06;
/// `MPI_EVENT_LINK_STATUS_CHANGE`.
pub const MPI_EVENT_LINK_STATUS_CHANGE: u32 = 0x07;
/// `MPI_EVENT_LOOP_STATE_CHANGE`.
pub const MPI_EVENT_LOOP_STATE_CHANGE: u32 = 0x08;
/// `MPI_EVENT_LOGOUT`.
pub const MPI_EVENT_LOGOUT: u32 = 0x09;
/// `MPI_EVENT_EVENT_CHANGE`.
pub const MPI_EVENT_EVENT_CHANGE: u32 = 0x0a;
/// `MPI_EVENT_INTEGRATED_RAID`.
pub const MPI_EVENT_INTEGRATED_RAID: u32 = 0x0b;
/// `MPI_EVENT_SCSI_DEVICE_STATUS_CHANGE`.
pub const MPI_EVENT_SCSI_DEVICE_STATUS_CHANGE: u32 = 0x0c;
/// `MPI_EVENT_ON_BUS_TIMER_EXPIRED`.
pub const MPI_EVENT_ON_BUS_TIMER_EXPIRED: u32 = 0x0d;
/// `MPI_EVENT_QUEUE_FULL`.
pub const MPI_EVENT_QUEUE_FULL: u32 = 0x0e;
/// `MPI_EVENT_SAS_DEVICE_STATUS_CHANGE`.
pub const MPI_EVENT_SAS_DEVICE_STATUS_CHANGE: u32 = 0x0f;
/// `MPI_EVENT_SAS_SES`.
pub const MPI_EVENT_SAS_SES: u32 = 0x10;
/// `MPI_EVENT_PERSISTENT_TABLE_FULL`.
pub const MPI_EVENT_PERSISTENT_TABLE_FULL: u32 = 0x11;
/// `MPI_EVENT_SAS_PHY_LINK_STATUS`.
pub const MPI_EVENT_SAS_PHY_LINK_STATUS: u32 = 0x12;
/// `MPI_EVENT_SAS_DISCOVERY_ERROR`.
pub const MPI_EVENT_SAS_DISCOVERY_ERROR: u32 = 0x13;
/// `MPI_EVENT_IR_RESYNC_UPDATE`.
pub const MPI_EVENT_IR_RESYNC_UPDATE: u32 = 0x14;
/// `MPI_EVENT_IR2`.
pub const MPI_EVENT_IR2: u32 = 0x15;
/// `MPI_EVENT_SAS_DISCOVERY`.
pub const MPI_EVENT_SAS_DISCOVERY: u32 = 0x16;
/// `MPI_EVENT_LOG_ENTRY_ADDED`.
pub const MPI_EVENT_LOG_ENTRY_ADDED: u32 = 0x21;
/// `MPI_WHOINIT_NOONE`.
pub const MPI_WHOINIT_NOONE: u8 = 0x00;
/// `MPI_WHOINIT_SYSTEM_BIOS`.
pub const MPI_WHOINIT_SYSTEM_BIOS: u8 = 0x01;
/// `MPI_WHOINIT_ROM_BIOS`.
pub const MPI_WHOINIT_ROM_BIOS: u8 = 0x02;
/// `MPI_WHOINIT_PCI_PEER`.
pub const MPI_WHOINIT_PCI_PEER: u8 = 0x03;
/// `MPI_WHOINIT_HOST_DRIVER`.
pub const MPI_WHOINIT_HOST_DRIVER: u8 = 0x04;
/// `MPI_WHOINIT_MANUFACTURER`.
pub const MPI_WHOINIT_MANUFACTURER: u8 = 0x05;
/// `MPI_PAGE_ADDRESS_FC_BTID`: Bus Target ID.
pub const MPI_PAGE_ADDRESS_FC_BTID: u32 = 1 << 24;
/// `struct mpi_msg_request`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgRequest {
    /// `reserved1`.
    pub reserved1: u8,
    /// `reserved2`.
    pub reserved2: u8,
    /// `chain_offset`.
    pub chain_offset: u8,
    /// `function`.
    pub function: u8,
    /// `reserved3`.
    pub reserved3: u8,
    /// `reserved4`.
    pub reserved4: u8,
    /// `reserved5`.
    pub reserved5: u8,
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgRequest>() == 12);
    assert!(core::mem::offset_of!(MpiMsgRequest, chain_offset) == 2);
    assert!(core::mem::offset_of!(MpiMsgRequest, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgRequest, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgRequest, msg_context) == 8);
};
/// `struct mpi_msg_reply`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgReply {
    /// `reserved1`.
    pub reserved1: u8,
    /// `reserved2`.
    pub reserved2: u8,
    /// `msg_length`.
    pub msg_length: u8,
    /// `function`.
    pub function: u8,
    /// `reserved3`.
    pub reserved3: u8,
    /// `reserved4`.
    pub reserved4: u8,
    /// `reserved5`.
    pub reserved5: u8,
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
    /// `reserved6`.
    pub reserved6: u8,
    /// `reserved7`.
    pub reserved7: u8,
    /// `ioc_status`.
    pub ioc_status: u16,
    /// `ioc_loginfo`.
    pub ioc_loginfo: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgReply>() == 20);
    assert!(core::mem::offset_of!(MpiMsgReply, msg_length) == 2);
    assert!(core::mem::offset_of!(MpiMsgReply, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgReply, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgReply, msg_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgReply, ioc_status) == 14);
    assert!(core::mem::offset_of!(MpiMsgReply, ioc_loginfo) == 16);
};
/// `MPI_IOCINIT_F_DISCARD_FW`.
pub const MPI_IOCINIT_F_DISCARD_FW: u8 = 1;
/// `MPI_IOCINIT_F_ENABLE_HOST_FIFO`.
pub const MPI_IOCINIT_F_ENABLE_HOST_FIFO: u8 = 1 << 1;
/// `MPI_IOCINIT_F_HOST_PG_BUF_PERSIST`.
pub const MPI_IOCINIT_F_HOST_PG_BUF_PERSIST: u8 = 1 << 2;
/// `struct mpi_msg_iocinit_request`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgIocinitRequest {
    /// `whoinit`.
    pub whoinit: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `chain_offset`.
    pub chain_offset: u8,
    /// `function`.
    pub function: u8,
    /// `flags`.
    pub flags: u8,
    /// `max_devices`.
    pub max_devices: u8,
    /// `max_buses`.
    pub max_buses: u8,
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
    /// `reply_frame_size`.
    pub reply_frame_size: u16,
    /// `reserved2`.
    pub reserved2: u16,
    /// `host_mfa_hi_addr`.
    pub host_mfa_hi_addr: u32,
    /// `sense_buffer_hi_addr`.
    pub sense_buffer_hi_addr: u32,
    /// `reply_fifo_host_signalling_addr`.
    pub reply_fifo_host_signalling_addr: u32,
    /// `host_page_buffer_sge`.
    pub host_page_buffer_sge: MpiSge,
    /// `msg_version_min`.
    pub msg_version_min: u8,
    /// `msg_version_maj`.
    pub msg_version_maj: u8,
    /// `hdr_version_unit`.
    pub hdr_version_unit: u8,
    /// `hdr_version_dev`.
    pub hdr_version_dev: u8,
}
const _: () = {
    assert!(size_of::<MpiMsgIocinitRequest>() == 44);
    assert!(core::mem::offset_of!(MpiMsgIocinitRequest, whoinit) == 0);
    assert!(core::mem::offset_of!(MpiMsgIocinitRequest, chain_offset) == 2);
    assert!(core::mem::offset_of!(MpiMsgIocinitRequest, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgIocinitRequest, flags) == 4);
    assert!(core::mem::offset_of!(MpiMsgIocinitRequest, max_devices) == 5);
    assert!(core::mem::offset_of!(MpiMsgIocinitRequest, max_buses) == 6);
    assert!(core::mem::offset_of!(MpiMsgIocinitRequest, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgIocinitRequest, msg_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgIocinitRequest, reply_frame_size) == 12);
    assert!(core::mem::offset_of!(MpiMsgIocinitRequest, host_mfa_hi_addr) == 16);
    assert!(core::mem::offset_of!(MpiMsgIocinitRequest, sense_buffer_hi_addr) == 20);
    assert!(core::mem::offset_of!(MpiMsgIocinitRequest, reply_fifo_host_signalling_addr) == 24);
    assert!(core::mem::offset_of!(MpiMsgIocinitRequest, host_page_buffer_sge) == 28);
    assert!(core::mem::offset_of!(MpiMsgIocinitRequest, msg_version_min) == 40);
    assert!(core::mem::offset_of!(MpiMsgIocinitRequest, msg_version_maj) == 41);
    assert!(core::mem::offset_of!(MpiMsgIocinitRequest, hdr_version_unit) == 42);
    assert!(core::mem::offset_of!(MpiMsgIocinitRequest, hdr_version_dev) == 43);
};
/// `struct mpi_msg_iocinit_reply`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgIocinitReply {
    /// `whoinit`.
    pub whoinit: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `msg_length`.
    pub msg_length: u8,
    /// `function`.
    pub function: u8,
    /// `flags`.
    pub flags: u8,
    /// `max_devices`.
    pub max_devices: u8,
    /// `max_buses`.
    pub max_buses: u8,
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
    /// `reserved2`.
    pub reserved2: u16,
    /// `ioc_status`.
    pub ioc_status: u16,
    /// `ioc_loginfo`.
    pub ioc_loginfo: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgIocinitReply>() == 20);
    assert!(core::mem::offset_of!(MpiMsgIocinitReply, whoinit) == 0);
    assert!(core::mem::offset_of!(MpiMsgIocinitReply, msg_length) == 2);
    assert!(core::mem::offset_of!(MpiMsgIocinitReply, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgIocinitReply, flags) == 4);
    assert!(core::mem::offset_of!(MpiMsgIocinitReply, max_devices) == 5);
    assert!(core::mem::offset_of!(MpiMsgIocinitReply, max_buses) == 6);
    assert!(core::mem::offset_of!(MpiMsgIocinitReply, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgIocinitReply, msg_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgIocinitReply, ioc_status) == 14);
    assert!(core::mem::offset_of!(MpiMsgIocinitReply, ioc_loginfo) == 16);
};
/// `struct mpi_msg_iocfacts_request`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgIocfactsRequest {
    /// `reserved1`.
    pub reserved1: u8,
    /// `reserved2`.
    pub reserved2: u8,
    /// `chain_offset`.
    pub chain_offset: u8,
    /// `function`.
    pub function: u8,
    /// `reserved3`.
    pub reserved3: u8,
    /// `reserved4`.
    pub reserved4: u8,
    /// `reserved5`.
    pub reserved5: u8,
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgIocfactsRequest>() == 12);
    assert!(core::mem::offset_of!(MpiMsgIocfactsRequest, chain_offset) == 2);
    assert!(core::mem::offset_of!(MpiMsgIocfactsRequest, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgIocfactsRequest, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgIocfactsRequest, msg_context) == 8);
};
/// `MPI_IOCFACTS_EXCEPT_CONFIG_CHECKSUM_FAIL`.
pub const MPI_IOCFACTS_EXCEPT_CONFIG_CHECKSUM_FAIL: u16 = 1;
/// `MPI_IOCFACTS_EXCEPT_RAID_CONFIG_INVALID`.
pub const MPI_IOCFACTS_EXCEPT_RAID_CONFIG_INVALID: u16 = 1 << 1;
/// `MPI_IOCFACTS_EXCEPT_FW_CHECKSUM_FAIL`.
pub const MPI_IOCFACTS_EXCEPT_FW_CHECKSUM_FAIL: u16 = 1 << 2;
/// `MPI_IOCFACTS_EXCEPT_PERSISTENT_TABLE_FULL`.
pub const MPI_IOCFACTS_EXCEPT_PERSISTENT_TABLE_FULL: u16 = 1 << 3;
/// `MPI_IOCFACTS_FLAGS_FW_DOWNLOAD_BOOT`.
pub const MPI_IOCFACTS_FLAGS_FW_DOWNLOAD_BOOT: u8 = 1;
/// `MPI_IOCFACTS_FLAGS_REPLY_FIFO_HOST_SIGNAL`.
pub const MPI_IOCFACTS_FLAGS_REPLY_FIFO_HOST_SIGNAL: u8 = 1 << 1;
/// `MPI_IOCFACTS_FLAGS_HOST_PAGE_BUFFER_PERSISTENT`.
pub const MPI_IOCFACTS_FLAGS_HOST_PAGE_BUFFER_PERSISTENT: u8 = 1 << 2;
/// `MPI_IOCFACTS_CAPABILITY_HIGH_PRI_Q`.
pub const MPI_IOCFACTS_CAPABILITY_HIGH_PRI_Q: u32 = 1;
/// `MPI_IOCFACTS_CAPABILITY_REPLY_HOST_SIGNAL`.
pub const MPI_IOCFACTS_CAPABILITY_REPLY_HOST_SIGNAL: u32 = 1 << 1;
/// `MPI_IOCFACTS_CAPABILITY_QUEUE_FULL_HANDLING`.
pub const MPI_IOCFACTS_CAPABILITY_QUEUE_FULL_HANDLING: u32 = 1 << 2;
/// `MPI_IOCFACTS_CAPABILITY_DIAG_TRACE_BUFFER`.
pub const MPI_IOCFACTS_CAPABILITY_DIAG_TRACE_BUFFER: u32 = 1 << 3;
/// `MPI_IOCFACTS_CAPABILITY_SNAPSHOT_BUFFER`.
pub const MPI_IOCFACTS_CAPABILITY_SNAPSHOT_BUFFER: u32 = 1 << 4;
/// `MPI_IOCFACTS_CAPABILITY_EXTENDED_BUFFER`.
pub const MPI_IOCFACTS_CAPABILITY_EXTENDED_BUFFER: u32 = 1 << 5;
/// `MPI_IOCFACTS_CAPABILITY_EEDP`.
pub const MPI_IOCFACTS_CAPABILITY_EEDP: u32 = 1 << 6;
/// `MPI_IOCFACTS_CAPABILITY_BIDIRECTIONAL`.
pub const MPI_IOCFACTS_CAPABILITY_BIDIRECTIONAL: u32 = 1 << 7;
/// `MPI_IOCFACTS_CAPABILITY_MULTICAST`.
pub const MPI_IOCFACTS_CAPABILITY_MULTICAST: u32 = 1 << 8;
/// `MPI_IOCFACTS_CAPABILITY_SCSIIO32`.
pub const MPI_IOCFACTS_CAPABILITY_SCSIIO32: u32 = 1 << 9;
/// `MPI_IOCFACTS_CAPABILITY_NO_SCSIIO16`.
pub const MPI_IOCFACTS_CAPABILITY_NO_SCSIIO16: u32 = 1 << 10;
/// `struct mpi_msg_iocfacts_reply`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgIocfactsReply {
    /// `msg_version_min`.
    pub msg_version_min: u8,
    /// `msg_version_maj`.
    pub msg_version_maj: u8,
    /// `msg_length`.
    pub msg_length: u8,
    /// `function`.
    pub function: u8,
    /// `header_version_min`.
    pub header_version_min: u8,
    /// `header_version_maj`.
    pub header_version_maj: u8,
    /// `ioc_number`.
    pub ioc_number: u8,
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
    /// `ioc_exceptions`.
    pub ioc_exceptions: u16,
    /// `ioc_status`.
    pub ioc_status: u16,
    /// `ioc_loginfo`.
    pub ioc_loginfo: u32,
    /// `max_chain_depth`.
    pub max_chain_depth: u8,
    /// `whoinit`.
    pub whoinit: u8,
    /// `block_size`.
    pub block_size: u8,
    /// `flags`.
    pub flags: u8,
    /// `reply_queue_depth`.
    pub reply_queue_depth: u16,
    /// `request_frame_size`.
    pub request_frame_size: u16,
    /// `reserved1`.
    pub reserved1: u16,
    /// `product_id`: product id.
    pub product_id: u16,
    /// `current_host_mfa_hi_addr`.
    pub current_host_mfa_hi_addr: u32,
    /// `global_credits`.
    pub global_credits: u16,
    /// `number_of_ports`.
    pub number_of_ports: u8,
    /// `event_state`.
    pub event_state: u8,
    /// `current_sense_buffer_hi_addr`.
    pub current_sense_buffer_hi_addr: u32,
    /// `current_reply_frame_size`.
    pub current_reply_frame_size: u16,
    /// `max_devices`.
    pub max_devices: u8,
    /// `max_buses`.
    pub max_buses: u8,
    /// `fw_image_size`.
    pub fw_image_size: u32,
    /// `ioc_capabilities`.
    pub ioc_capabilities: u32,
    /// `fw_version_dev`.
    pub fw_version_dev: u8,
    /// `fw_version_unit`.
    pub fw_version_unit: u8,
    /// `fw_version_min`.
    pub fw_version_min: u8,
    /// `fw_version_maj`.
    pub fw_version_maj: u8,
    /// `hi_priority_queue_depth`.
    pub hi_priority_queue_depth: u16,
    /// `reserved2`.
    pub reserved2: u16,
    /// `host_page_buffer_sge`.
    pub host_page_buffer_sge: MpiSge,
    /// `reply_fifo_host_signalling_addr`.
    pub reply_fifo_host_signalling_addr: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgIocfactsReply>() == 80);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, msg_version_min) == 0);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, msg_version_maj) == 1);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, msg_length) == 2);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, header_version_min) == 4);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, header_version_maj) == 5);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, ioc_number) == 6);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, msg_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, ioc_exceptions) == 12);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, ioc_status) == 14);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, ioc_loginfo) == 16);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, max_chain_depth) == 20);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, whoinit) == 21);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, block_size) == 22);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, flags) == 23);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, reply_queue_depth) == 24);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, request_frame_size) == 26);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, product_id) == 30);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, current_host_mfa_hi_addr) == 32);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, global_credits) == 36);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, number_of_ports) == 38);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, event_state) == 39);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, current_sense_buffer_hi_addr) == 40);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, current_reply_frame_size) == 44);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, max_devices) == 46);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, max_buses) == 47);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, fw_image_size) == 48);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, ioc_capabilities) == 52);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, fw_version_dev) == 56);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, fw_version_unit) == 57);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, fw_version_min) == 58);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, fw_version_maj) == 59);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, hi_priority_queue_depth) == 60);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, host_page_buffer_sge) == 64);
    assert!(core::mem::offset_of!(MpiMsgIocfactsReply, reply_fifo_host_signalling_addr) == 76);
};
/// `struct mpi_msg_portfacts_request`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgPortfactsRequest {
    /// `reserved1`.
    pub reserved1: u8,
    /// `reserved2`.
    pub reserved2: u8,
    /// `chain_offset`.
    pub chain_offset: u8,
    /// `function`.
    pub function: u8,
    /// `reserved3`.
    pub reserved3: u8,
    /// `reserved4`.
    pub reserved4: u8,
    /// `port_number`.
    pub port_number: u8,
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgPortfactsRequest>() == 12);
    assert!(core::mem::offset_of!(MpiMsgPortfactsRequest, chain_offset) == 2);
    assert!(core::mem::offset_of!(MpiMsgPortfactsRequest, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgPortfactsRequest, port_number) == 6);
    assert!(core::mem::offset_of!(MpiMsgPortfactsRequest, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgPortfactsRequest, msg_context) == 8);
};
/// `MPI_PORTFACTS_PORTTYPE_INACTIVE`.
pub const MPI_PORTFACTS_PORTTYPE_INACTIVE: u8 = 0x00;
/// `MPI_PORTFACTS_PORTTYPE_SCSI`.
pub const MPI_PORTFACTS_PORTTYPE_SCSI: u8 = 0x01;
/// `MPI_PORTFACTS_PORTTYPE_FC`.
pub const MPI_PORTFACTS_PORTTYPE_FC: u8 = 0x10;
/// `MPI_PORTFACTS_PORTTYPE_ISCSI`.
pub const MPI_PORTFACTS_PORTTYPE_ISCSI: u8 = 0x20;
/// `MPI_PORTFACTS_PORTTYPE_SAS`.
pub const MPI_PORTFACTS_PORTTYPE_SAS: u8 = 0x30;
/// `MPI_PORTFACTS_PROTOCOL_LOGBUSADDR`.
pub const MPI_PORTFACTS_PROTOCOL_LOGBUSADDR: u16 = 1;
/// `MPI_PORTFACTS_PROTOCOL_LAN`.
pub const MPI_PORTFACTS_PROTOCOL_LAN: u16 = 1 << 1;
/// `MPI_PORTFACTS_PROTOCOL_TARGET`.
pub const MPI_PORTFACTS_PROTOCOL_TARGET: u16 = 1 << 2;
/// `MPI_PORTFACTS_PROTOCOL_INITIATOR`.
pub const MPI_PORTFACTS_PROTOCOL_INITIATOR: u16 = 1 << 3;
/// `struct mpi_msg_portfacts_reply`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgPortfactsReply {
    /// `reserved1`.
    pub reserved1: u16,
    /// `msg_length`.
    pub msg_length: u8,
    /// `function`.
    pub function: u8,
    /// `reserved2`.
    pub reserved2: u16,
    /// `port_number`.
    pub port_number: u8,
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
    /// `reserved3`.
    pub reserved3: u16,
    /// `ioc_status`.
    pub ioc_status: u16,
    /// `ioc_loginfo`.
    pub ioc_loginfo: u32,
    /// `reserved4`.
    pub reserved4: u8,
    /// `port_type`.
    pub port_type: u8,
    /// `max_devices`.
    pub max_devices: u16,
    /// `port_scsi_id`.
    pub port_scsi_id: u16,
    /// `protocol_flags`.
    pub protocol_flags: u16,
    /// `max_posted_cmd_buffers`.
    pub max_posted_cmd_buffers: u16,
    /// `max_persistent_ids`.
    pub max_persistent_ids: u16,
    /// `max_lan_buckets`.
    pub max_lan_buckets: u16,
    /// `reserved5`.
    pub reserved5: u16,
    /// `reserved6`.
    pub reserved6: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgPortfactsReply>() == 40);
    assert!(core::mem::offset_of!(MpiMsgPortfactsReply, msg_length) == 2);
    assert!(core::mem::offset_of!(MpiMsgPortfactsReply, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgPortfactsReply, port_number) == 6);
    assert!(core::mem::offset_of!(MpiMsgPortfactsReply, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgPortfactsReply, msg_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgPortfactsReply, ioc_status) == 14);
    assert!(core::mem::offset_of!(MpiMsgPortfactsReply, ioc_loginfo) == 16);
    assert!(core::mem::offset_of!(MpiMsgPortfactsReply, port_type) == 21);
    assert!(core::mem::offset_of!(MpiMsgPortfactsReply, max_devices) == 22);
    assert!(core::mem::offset_of!(MpiMsgPortfactsReply, port_scsi_id) == 24);
    assert!(core::mem::offset_of!(MpiMsgPortfactsReply, protocol_flags) == 26);
    assert!(core::mem::offset_of!(MpiMsgPortfactsReply, max_posted_cmd_buffers) == 28);
    assert!(core::mem::offset_of!(MpiMsgPortfactsReply, max_persistent_ids) == 30);
    assert!(core::mem::offset_of!(MpiMsgPortfactsReply, max_lan_buckets) == 32);
};
/// `struct mpi_msg_portenable_request`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgPortenableRequest {
    /// `reserved1`.
    pub reserved1: u16,
    /// `chain_offset`.
    pub chain_offset: u8,
    /// `function`.
    pub function: u8,
    /// `reserved2`.
    pub reserved2: u16,
    /// `port_number`.
    pub port_number: u8,
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgPortenableRequest>() == 12);
    assert!(core::mem::offset_of!(MpiMsgPortenableRequest, chain_offset) == 2);
    assert!(core::mem::offset_of!(MpiMsgPortenableRequest, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgPortenableRequest, port_number) == 6);
    assert!(core::mem::offset_of!(MpiMsgPortenableRequest, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgPortenableRequest, msg_context) == 8);
};
/// `struct mpi_msg_portenable_reply`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgPortenableReply {
    /// `reserved1`.
    pub reserved1: u16,
    /// `msg_length`.
    pub msg_length: u8,
    /// `function`.
    pub function: u8,
    /// `reserved2`.
    pub reserved2: u16,
    /// `port_number`.
    pub port_number: u8,
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
    /// `reserved3`.
    pub reserved3: u16,
    /// `ioc_status`.
    pub ioc_status: u16,
    /// `ioc_loginfo`.
    pub ioc_loginfo: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgPortenableReply>() == 20);
    assert!(core::mem::offset_of!(MpiMsgPortenableReply, msg_length) == 2);
    assert!(core::mem::offset_of!(MpiMsgPortenableReply, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgPortenableReply, port_number) == 6);
    assert!(core::mem::offset_of!(MpiMsgPortenableReply, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgPortenableReply, msg_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgPortenableReply, ioc_status) == 14);
    assert!(core::mem::offset_of!(MpiMsgPortenableReply, ioc_loginfo) == 16);
};
/// `MPI_EVENT_SWITCH_ON`.
pub const MPI_EVENT_SWITCH_ON: u8 = 0x01;
/// `MPI_EVENT_SWITCH_OFF`.
pub const MPI_EVENT_SWITCH_OFF: u8 = 0x00;
/// `struct mpi_msg_event_request`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgEventRequest {
    /// `event_switch`.
    pub event_switch: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `chain_offset`.
    pub chain_offset: u8,
    /// `function`.
    pub function: u8,
    /// `reserved2`.
    pub reserved2: [u8; 3],
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgEventRequest>() == 12);
    assert!(core::mem::offset_of!(MpiMsgEventRequest, event_switch) == 0);
    assert!(core::mem::offset_of!(MpiMsgEventRequest, chain_offset) == 2);
    assert!(core::mem::offset_of!(MpiMsgEventRequest, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgEventRequest, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgEventRequest, msg_context) == 8);
};
/// `MPI_EVENT_ACK_REQUIRED`.
pub const MPI_EVENT_ACK_REQUIRED: u8 = 0x01;
/// `MPI_EVENT_FLAGS_REPLY_KEPT`.
pub const MPI_EVENT_FLAGS_REPLY_KEPT: u8 = 1 << 7;
/// `struct mpi_msg_event_reply`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgEventReply {
    /// `data_length`.
    pub data_length: u16,
    /// `msg_length`.
    pub msg_length: u8,
    /// `function`.
    pub function: u8,
    /// `reserved1`.
    pub reserved1: u16,
    /// `ack_required`.
    pub ack_required: u8,
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
    /// `reserved2`.
    pub reserved2: u16,
    /// `ioc_status`.
    pub ioc_status: u16,
    /// `ioc_loginfo`.
    pub ioc_loginfo: u32,
    /// `event`.
    pub event: u32,
    /// `event_context`.
    pub event_context: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgEventReply>() == 28);
    assert!(core::mem::offset_of!(MpiMsgEventReply, data_length) == 0);
    assert!(core::mem::offset_of!(MpiMsgEventReply, msg_length) == 2);
    assert!(core::mem::offset_of!(MpiMsgEventReply, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgEventReply, ack_required) == 6);
    assert!(core::mem::offset_of!(MpiMsgEventReply, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgEventReply, msg_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgEventReply, ioc_status) == 14);
    assert!(core::mem::offset_of!(MpiMsgEventReply, ioc_loginfo) == 16);
    assert!(core::mem::offset_of!(MpiMsgEventReply, event) == 20);
    assert!(core::mem::offset_of!(MpiMsgEventReply, event_context) == 24);
};
/// `struct mpi_evt_change`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiEvtChange {
    /// `event_state`.
    pub event_state: u8,
    /// `reserved`.
    pub reserved: [u8; 3],
}
const _: () = {
    assert!(size_of::<MpiEvtChange>() == 4);
    assert!(core::mem::offset_of!(MpiEvtChange, event_state) == 0);
};
/// `MPI_EVT_LINK_STATUS_CHANGE_OFFLINE`.
pub const MPI_EVT_LINK_STATUS_CHANGE_OFFLINE: u8 = 0x00;
/// `MPI_EVT_LINK_STATUS_CHANGE_ACTIVE`.
pub const MPI_EVT_LINK_STATUS_CHANGE_ACTIVE: u8 = 0x01;
/// `struct mpi_evt_link_status_change`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiEvtLinkStatusChange {
    /// `state`.
    pub state: u8,
    /// `_reserved1`.
    pub _reserved1: [u8; 3],
    /// `_reserved2`.
    pub _reserved2: [u8; 1],
    /// `port`.
    pub port: u8,
    /// `_reserved3`.
    pub _reserved3: [u8; 2],
}
const _: () = {
    assert!(size_of::<MpiEvtLinkStatusChange>() == 8);
    assert!(core::mem::offset_of!(MpiEvtLinkStatusChange, state) == 0);
    assert!(core::mem::offset_of!(MpiEvtLinkStatusChange, port) == 5);
};
/// `MPI_EVT_LOOP_STATUS_CHANGE_TYPE_LIP`.
pub const MPI_EVT_LOOP_STATUS_CHANGE_TYPE_LIP: u8 = 0x01;
/// `MPI_EVT_LOOP_STATUS_CHANGE_TYPE_LPE`.
pub const MPI_EVT_LOOP_STATUS_CHANGE_TYPE_LPE: u8 = 0x02;
/// `MPI_EVT_LOOP_STATUS_CHANGE_TYPE_LPB`.
pub const MPI_EVT_LOOP_STATUS_CHANGE_TYPE_LPB: u8 = 0x03;
/// `struct mpi_evt_loop_status_change`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiEvtLoopStatusChange {
    /// `character4`.
    pub character4: u8,
    /// `character3`.
    pub character3: u8,
    /// `type`.
    pub r#type: u8,
    /// `_reserved1`.
    pub _reserved1: [u8; 1],
    /// `_reserved2`.
    pub _reserved2: [u8; 1],
    /// `port`.
    pub port: u8,
    /// `_reserved3`.
    pub _reserved3: [u8; 2],
}
const _: () = {
    assert!(size_of::<MpiEvtLoopStatusChange>() == 8);
    assert!(core::mem::offset_of!(MpiEvtLoopStatusChange, character4) == 0);
    assert!(core::mem::offset_of!(MpiEvtLoopStatusChange, character3) == 1);
    assert!(core::mem::offset_of!(MpiEvtLoopStatusChange, r#type) == 2);
    assert!(core::mem::offset_of!(MpiEvtLoopStatusChange, port) == 5);
};
/// `struct mpi_evt_logout`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiEvtLogout {
    /// `n_portid`.
    pub n_portid: u32,
    /// `alias_index`.
    pub alias_index: u8,
    /// `port`.
    pub port: u8,
    /// `_reserved`.
    pub _reserved: [u8; 2],
}
const _: () = {
    assert!(size_of::<MpiEvtLogout>() == 8);
    assert!(core::mem::offset_of!(MpiEvtLogout, n_portid) == 0);
    assert!(core::mem::offset_of!(MpiEvtLogout, alias_index) == 4);
    assert!(core::mem::offset_of!(MpiEvtLogout, port) == 5);
};
/// `MPI_EVT_SASPHY_LINK_CUR(x)`.
pub const fn mpi_evt_sasphy_link_cur(x: u8) -> u8 {
    (x & 0xf0) >> 4
}
/// `MPI_EVT_SASPHY_LINK_PREV(x)`.
pub const fn mpi_evt_sasphy_link_prev(x: u8) -> u8 {
    x & 0x0f
}
/// `MPI_EVT_SASPHY_LINK_ENABLED`.
pub const MPI_EVT_SASPHY_LINK_ENABLED: u8 = 0x0;
/// `MPI_EVT_SASPHY_LINK_DISABLED`.
pub const MPI_EVT_SASPHY_LINK_DISABLED: u8 = 0x1;
/// `MPI_EVT_SASPHY_LINK_NEGFAIL`.
pub const MPI_EVT_SASPHY_LINK_NEGFAIL: u8 = 0x2;
/// `MPI_EVT_SASPHY_LINK_SATAOOB`.
pub const MPI_EVT_SASPHY_LINK_SATAOOB: u8 = 0x3;
/// `MPI_EVT_SASPHY_LINK_1_5GBPS`.
pub const MPI_EVT_SASPHY_LINK_1_5GBPS: u8 = 0x8;
/// `MPI_EVT_SASPHY_LINK_3_0GBPS`.
pub const MPI_EVT_SASPHY_LINK_3_0GBPS: u8 = 0x9;
/// `struct mpi_evt_sas_phy`.
#[derive(Clone, Copy, Default)]
#[repr(C, packed)]
pub struct MpiEvtSasPhy {
    /// `phy_num`.
    pub phy_num: u8,
    /// `link_rates`.
    pub link_rates: u8,
    /// `dev_handle`.
    pub dev_handle: u16,
    /// `sas_addr`.
    pub sas_addr: u64,
}
const _: () = {
    assert!(size_of::<MpiEvtSasPhy>() == 12);
    assert!(core::mem::offset_of!(MpiEvtSasPhy, phy_num) == 0);
    assert!(core::mem::offset_of!(MpiEvtSasPhy, link_rates) == 1);
    assert!(core::mem::offset_of!(MpiEvtSasPhy, dev_handle) == 2);
    assert!(core::mem::offset_of!(MpiEvtSasPhy, sas_addr) == 4);
};
/// `MPI_EVT_SASCH_REASON_ADDED`.
pub const MPI_EVT_SASCH_REASON_ADDED: u8 = 0x03;
/// `MPI_EVT_SASCH_REASON_NOT_RESPONDING`.
pub const MPI_EVT_SASCH_REASON_NOT_RESPONDING: u8 = 0x04;
/// `MPI_EVT_SASCH_REASON_SMART_DATA`.
pub const MPI_EVT_SASCH_REASON_SMART_DATA: u8 = 0x05;
/// `MPI_EVT_SASCH_REASON_NO_PERSIST_ADDED`.
pub const MPI_EVT_SASCH_REASON_NO_PERSIST_ADDED: u8 = 0x06;
/// `MPI_EVT_SASCH_REASON_UNSUPPORTED`.
pub const MPI_EVT_SASCH_REASON_UNSUPPORTED: u8 = 0x07;
/// `MPI_EVT_SASCH_REASON_INTERNAL_RESET`.
pub const MPI_EVT_SASCH_REASON_INTERNAL_RESET: u8 = 0x08;
/// `MPI_EVT_SASCH_INFO_ATAPI`.
pub const MPI_EVT_SASCH_INFO_ATAPI: u32 = 1 << 13;
/// `MPI_EVT_SASCH_INFO_LSI`.
pub const MPI_EVT_SASCH_INFO_LSI: u32 = 1 << 12;
/// `MPI_EVT_SASCH_INFO_DIRECT_ATTACHED`.
pub const MPI_EVT_SASCH_INFO_DIRECT_ATTACHED: u32 = 1 << 11;
/// `MPI_EVT_SASCH_INFO_SSP`.
pub const MPI_EVT_SASCH_INFO_SSP: u32 = 1 << 10;
/// `MPI_EVT_SASCH_INFO_STP`.
pub const MPI_EVT_SASCH_INFO_STP: u32 = 1 << 9;
/// `MPI_EVT_SASCH_INFO_SMP`.
pub const MPI_EVT_SASCH_INFO_SMP: u32 = 1 << 8;
/// `MPI_EVT_SASCH_INFO_SATA`.
pub const MPI_EVT_SASCH_INFO_SATA: u32 = 1 << 7;
/// `MPI_EVT_SASCH_INFO_SSP_INITIATOR`.
pub const MPI_EVT_SASCH_INFO_SSP_INITIATOR: u32 = 1 << 6;
/// `MPI_EVT_SASCH_INFO_STP_INITIATOR`.
pub const MPI_EVT_SASCH_INFO_STP_INITIATOR: u32 = 1 << 5;
/// `MPI_EVT_SASCH_INFO_SMP_INITIATOR`.
pub const MPI_EVT_SASCH_INFO_SMP_INITIATOR: u32 = 1 << 4;
/// `MPI_EVT_SASCH_INFO_SATA_HOST`.
pub const MPI_EVT_SASCH_INFO_SATA_HOST: u32 = 1 << 3;
/// `MPI_EVT_SASCH_INFO_TYPE_MASK`.
pub const MPI_EVT_SASCH_INFO_TYPE_MASK: u32 = 0x7;
/// `MPI_EVT_SASCH_INFO_TYPE_NONE`.
pub const MPI_EVT_SASCH_INFO_TYPE_NONE: u32 = 0x0;
/// `MPI_EVT_SASCH_INFO_TYPE_END`.
pub const MPI_EVT_SASCH_INFO_TYPE_END: u32 = 0x1;
/// `MPI_EVT_SASCH_INFO_TYPE_EDGE`.
pub const MPI_EVT_SASCH_INFO_TYPE_EDGE: u32 = 0x2;
/// `MPI_EVT_SASCH_INFO_TYPE_FANOUT`.
pub const MPI_EVT_SASCH_INFO_TYPE_FANOUT: u32 = 0x3;
/// `struct mpi_evt_sas_change`.
#[derive(Clone, Copy, Default)]
#[repr(C, packed)]
pub struct MpiEvtSasChange {
    /// `target`.
    pub target: u8,
    /// `bus`.
    pub bus: u8,
    /// `reason`.
    pub reason: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `asc`.
    pub asc: u8,
    /// `ascq`.
    pub ascq: u8,
    /// `dev_handle`.
    pub dev_handle: u16,
    /// `device_info`.
    pub device_info: u32,
    /// `parent_dev_handle`.
    pub parent_dev_handle: u16,
    /// `phy_num`.
    pub phy_num: u8,
    /// `reserved2`.
    pub reserved2: u8,
    /// `sas_addr`.
    pub sas_addr: u64,
}
const _: () = {
    assert!(size_of::<MpiEvtSasChange>() == 24);
    assert!(core::mem::offset_of!(MpiEvtSasChange, target) == 0);
    assert!(core::mem::offset_of!(MpiEvtSasChange, bus) == 1);
    assert!(core::mem::offset_of!(MpiEvtSasChange, reason) == 2);
    assert!(core::mem::offset_of!(MpiEvtSasChange, asc) == 4);
    assert!(core::mem::offset_of!(MpiEvtSasChange, ascq) == 5);
    assert!(core::mem::offset_of!(MpiEvtSasChange, dev_handle) == 6);
    assert!(core::mem::offset_of!(MpiEvtSasChange, device_info) == 8);
    assert!(core::mem::offset_of!(MpiEvtSasChange, parent_dev_handle) == 12);
    assert!(core::mem::offset_of!(MpiEvtSasChange, phy_num) == 14);
    assert!(core::mem::offset_of!(MpiEvtSasChange, sas_addr) == 16);
};
/// `struct mpi_msg_eventack_request`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgEventackRequest {
    /// `reserved1`.
    pub reserved1: u16,
    /// `chain_offset`.
    pub chain_offset: u8,
    /// `function`.
    pub function: u8,
    /// `reserved2`.
    pub reserved2: [u8; 3],
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
    /// `event`.
    pub event: u32,
    /// `event_context`.
    pub event_context: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgEventackRequest>() == 20);
    assert!(core::mem::offset_of!(MpiMsgEventackRequest, chain_offset) == 2);
    assert!(core::mem::offset_of!(MpiMsgEventackRequest, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgEventackRequest, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgEventackRequest, msg_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgEventackRequest, event) == 12);
    assert!(core::mem::offset_of!(MpiMsgEventackRequest, event_context) == 16);
};
/// `struct mpi_msg_eventack_reply`.
#[derive(Clone, Copy, Default)]
#[repr(C, packed)]
pub struct MpiMsgEventackReply {
    /// `reserved1`.
    pub reserved1: u16,
    /// `msg_length`.
    pub msg_length: u8,
    /// `function`.
    pub function: u8,
    /// `reserved2`.
    pub reserved2: [u8; 3],
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
    /// `reserved3`.
    pub reserved3: u16,
    /// `ioc_status`.
    pub ioc_status: u32,
    /// `ioc_loginfo`.
    pub ioc_loginfo: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgEventackReply>() == 22);
    assert!(core::mem::offset_of!(MpiMsgEventackReply, msg_length) == 2);
    assert!(core::mem::offset_of!(MpiMsgEventackReply, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgEventackReply, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgEventackReply, msg_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgEventackReply, ioc_status) == 14);
    assert!(core::mem::offset_of!(MpiMsgEventackReply, ioc_loginfo) == 18);
};
/// `MPI_FWUPLOAD_IMAGETYPE_IOC_FW`.
pub const MPI_FWUPLOAD_IMAGETYPE_IOC_FW: u8 = 0x00;
/// `MPI_FWUPLOAD_IMAGETYPE_NV_FW`.
pub const MPI_FWUPLOAD_IMAGETYPE_NV_FW: u8 = 0x01;
/// `MPI_FWUPLOAD_IMAGETYPE_MPI_NV_FW`.
pub const MPI_FWUPLOAD_IMAGETYPE_MPI_NV_FW: u8 = 0x02;
/// `MPI_FWUPLOAD_IMAGETYPE_NV_DATA`.
pub const MPI_FWUPLOAD_IMAGETYPE_NV_DATA: u8 = 0x03;
/// `MPI_FWUPLOAD_IMAGETYPE_BOOT`.
pub const MPI_FWUPLOAD_IMAGETYPE_BOOT: u8 = 0x04;
/// `MPI_FWUPLOAD_IMAGETYPE_NV_BACKUP`.
pub const MPI_FWUPLOAD_IMAGETYPE_NV_BACKUP: u8 = 0x05;
/// `struct mpi_msg_fwupload_request`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgFwuploadRequest {
    /// `image_type`.
    pub image_type: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `chain_offset`.
    pub chain_offset: u8,
    /// `function`.
    pub function: u8,
    /// `reserved2`.
    pub reserved2: [u8; 3],
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
    /// `tce`.
    pub tce: MpiFwTce,
}
const _: () = {
    assert!(size_of::<MpiMsgFwuploadRequest>() == 28);
    assert!(core::mem::offset_of!(MpiMsgFwuploadRequest, image_type) == 0);
    assert!(core::mem::offset_of!(MpiMsgFwuploadRequest, chain_offset) == 2);
    assert!(core::mem::offset_of!(MpiMsgFwuploadRequest, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgFwuploadRequest, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgFwuploadRequest, msg_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgFwuploadRequest, tce) == 12);
};
/// `struct mpi_msg_fwupload_reply`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgFwuploadReply {
    /// `image_type`.
    pub image_type: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `msg_length`.
    pub msg_length: u8,
    /// `function`.
    pub function: u8,
    /// `reserved2`.
    pub reserved2: [u8; 3],
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
    /// `reserved3`.
    pub reserved3: u16,
    /// `ioc_status`.
    pub ioc_status: u16,
    /// `ioc_loginfo`.
    pub ioc_loginfo: u32,
    /// `actual_image_size`.
    pub actual_image_size: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgFwuploadReply>() == 24);
    assert!(core::mem::offset_of!(MpiMsgFwuploadReply, image_type) == 0);
    assert!(core::mem::offset_of!(MpiMsgFwuploadReply, msg_length) == 2);
    assert!(core::mem::offset_of!(MpiMsgFwuploadReply, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgFwuploadReply, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgFwuploadReply, msg_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgFwuploadReply, ioc_status) == 14);
    assert!(core::mem::offset_of!(MpiMsgFwuploadReply, ioc_loginfo) == 16);
    assert!(core::mem::offset_of!(MpiMsgFwuploadReply, actual_image_size) == 20);
};
/// `MPI_SCSIIO_EEDP`.
pub const MPI_SCSIIO_EEDP: u8 = 0xf0;
/// `MPI_SCSIIO_CMD_DATA_DIR`.
pub const MPI_SCSIIO_CMD_DATA_DIR: u8 = 1 << 2;
/// `MPI_SCSIIO_SENSE_BUF_LOC`.
pub const MPI_SCSIIO_SENSE_BUF_LOC: u8 = 1 << 1;
/// `MPI_SCSIIO_SENSE_BUF_ADDR_WIDTH`.
pub const MPI_SCSIIO_SENSE_BUF_ADDR_WIDTH: u8 = 1;
/// `MPI_SCSIIO_SENSE_BUF_ADDR_WIDTH_32`.
pub const MPI_SCSIIO_SENSE_BUF_ADDR_WIDTH_32: u8 = 0;
/// `MPI_SCSIIO_SENSE_BUF_ADDR_WIDTH_64`.
pub const MPI_SCSIIO_SENSE_BUF_ADDR_WIDTH_64: u8 = 1;
/// `MPI_SCSIIO_ATTR_SIMPLE_Q`.
pub const MPI_SCSIIO_ATTR_SIMPLE_Q: u8 = 0x0;
/// `MPI_SCSIIO_ATTR_HEAD_OF_Q`.
pub const MPI_SCSIIO_ATTR_HEAD_OF_Q: u8 = 0x1;
/// `MPI_SCSIIO_ATTR_ORDERED_Q`.
pub const MPI_SCSIIO_ATTR_ORDERED_Q: u8 = 0x2;
/// `MPI_SCSIIO_ATTR_ACA_Q`.
pub const MPI_SCSIIO_ATTR_ACA_Q: u8 = 0x4;
/// `MPI_SCSIIO_ATTR_UNTAGGED`.
pub const MPI_SCSIIO_ATTR_UNTAGGED: u8 = 0x5;
/// `MPI_SCSIIO_ATTR_NO_DISCONNECT`.
pub const MPI_SCSIIO_ATTR_NO_DISCONNECT: u8 = 0x7;
/// `MPI_SCSIIO_DIR_NONE`.
pub const MPI_SCSIIO_DIR_NONE: u8 = 0x0;
/// `MPI_SCSIIO_DIR_WRITE`.
pub const MPI_SCSIIO_DIR_WRITE: u8 = 0x1;
/// `MPI_SCSIIO_DIR_READ`.
pub const MPI_SCSIIO_DIR_READ: u8 = 0x2;
/// `MPI_CDB_LEN`.
pub const MPI_CDB_LEN: usize = 16;
/// `struct mpi_msg_scsi_io`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgScsiIo {
    /// `target_id`.
    pub target_id: u8,
    /// `bus`.
    pub bus: u8,
    /// `chain_offset`.
    pub chain_offset: u8,
    /// `function`.
    pub function: u8,
    /// `cdb_length`.
    pub cdb_length: u8,
    /// `sense_buf_len`.
    pub sense_buf_len: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
    /// `lun`.
    pub lun: [u16; 4],
    /// `reserved2`.
    pub reserved2: u8,
    /// `tagging`.
    pub tagging: u8,
    /// `reserved3`.
    pub reserved3: u8,
    /// `direction`.
    pub direction: u8,
    /// `cdb`.
    pub cdb: [u8; MPI_CDB_LEN],
    /// `data_length`.
    pub data_length: u32,
    /// `sense_buf_low_addr`.
    pub sense_buf_low_addr: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgScsiIo>() == 48);
    assert!(core::mem::offset_of!(MpiMsgScsiIo, target_id) == 0);
    assert!(core::mem::offset_of!(MpiMsgScsiIo, bus) == 1);
    assert!(core::mem::offset_of!(MpiMsgScsiIo, chain_offset) == 2);
    assert!(core::mem::offset_of!(MpiMsgScsiIo, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgScsiIo, cdb_length) == 4);
    assert!(core::mem::offset_of!(MpiMsgScsiIo, sense_buf_len) == 5);
    assert!(core::mem::offset_of!(MpiMsgScsiIo, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgScsiIo, msg_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgScsiIo, lun) == 12);
    assert!(core::mem::offset_of!(MpiMsgScsiIo, tagging) == 21);
    assert!(core::mem::offset_of!(MpiMsgScsiIo, direction) == 23);
    assert!(core::mem::offset_of!(MpiMsgScsiIo, cdb) == 24);
    assert!(core::mem::offset_of!(MpiMsgScsiIo, data_length) == 40);
    assert!(core::mem::offset_of!(MpiMsgScsiIo, sense_buf_low_addr) == 44);
};
/// `MPI_SCSIIO_ERR_STATE_AUTOSENSE_VALID`.
pub const MPI_SCSIIO_ERR_STATE_AUTOSENSE_VALID: u8 = 1;
/// `MPI_SCSIIO_ERR_STATE_AUTOSENSE_FAILED`.
pub const MPI_SCSIIO_ERR_STATE_AUTOSENSE_FAILED: u8 = 1 << 2;
/// `MPI_SCSIIO_ERR_STATE_NO_SCSI_STATUS`.
pub const MPI_SCSIIO_ERR_STATE_NO_SCSI_STATUS: u8 = 1 << 3;
/// `MPI_SCSIIO_ERR_STATE_TERMINATED`.
pub const MPI_SCSIIO_ERR_STATE_TERMINATED: u8 = 1 << 4;
/// `MPI_SCSIIO_ERR_STATE_RESPONSE_INFO_VALID`.
pub const MPI_SCSIIO_ERR_STATE_RESPONSE_INFO_VALID: u8 = 1 << 5;
/// `MPI_SCSIIO_ERR_STATE_QUEUE_TAG_REJECTED`.
pub const MPI_SCSIIO_ERR_STATE_QUEUE_TAG_REJECTED: u8 = 1 << 6;
/// `struct mpi_msg_scsi_io_error`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgScsiIoError {
    /// `target_id`.
    pub target_id: u8,
    /// `bus`.
    pub bus: u8,
    /// `msg_length`.
    pub msg_length: u8,
    /// `function`.
    pub function: u8,
    /// `cdb_length`.
    pub cdb_length: u8,
    /// `sense_buf_len`.
    pub sense_buf_len: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
    /// `scsi_status`.
    pub scsi_status: u8,
    /// `scsi_state`.
    pub scsi_state: u8,
    /// `ioc_status`.
    pub ioc_status: u16,
    /// `ioc_loginfo`.
    pub ioc_loginfo: u32,
    /// `transfer_count`.
    pub transfer_count: u32,
    /// `sense_count`.
    pub sense_count: u32,
    /// `response_info`.
    pub response_info: u32,
    /// `tag`.
    pub tag: u16,
    /// `reserved2`.
    pub reserved2: u16,
}
const _: () = {
    assert!(size_of::<MpiMsgScsiIoError>() == 36);
    assert!(core::mem::offset_of!(MpiMsgScsiIoError, target_id) == 0);
    assert!(core::mem::offset_of!(MpiMsgScsiIoError, bus) == 1);
    assert!(core::mem::offset_of!(MpiMsgScsiIoError, msg_length) == 2);
    assert!(core::mem::offset_of!(MpiMsgScsiIoError, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgScsiIoError, cdb_length) == 4);
    assert!(core::mem::offset_of!(MpiMsgScsiIoError, sense_buf_len) == 5);
    assert!(core::mem::offset_of!(MpiMsgScsiIoError, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgScsiIoError, msg_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgScsiIoError, scsi_status) == 12);
    assert!(core::mem::offset_of!(MpiMsgScsiIoError, scsi_state) == 13);
    assert!(core::mem::offset_of!(MpiMsgScsiIoError, ioc_status) == 14);
    assert!(core::mem::offset_of!(MpiMsgScsiIoError, ioc_loginfo) == 16);
    assert!(core::mem::offset_of!(MpiMsgScsiIoError, transfer_count) == 20);
    assert!(core::mem::offset_of!(MpiMsgScsiIoError, sense_count) == 24);
    assert!(core::mem::offset_of!(MpiMsgScsiIoError, response_info) == 28);
    assert!(core::mem::offset_of!(MpiMsgScsiIoError, tag) == 32);
};
/// `MPI_MSG_SCSI_TASK_TYPE_ABORT_TASK`.
pub const MPI_MSG_SCSI_TASK_TYPE_ABORT_TASK: u8 = 0x01;
/// `MPI_MSG_SCSI_TASK_TYPE_ABRT_TASK_SET`.
pub const MPI_MSG_SCSI_TASK_TYPE_ABRT_TASK_SET: u8 = 0x02;
/// `MPI_MSG_SCSI_TASK_TYPE_TARGET_RESET`.
pub const MPI_MSG_SCSI_TASK_TYPE_TARGET_RESET: u8 = 0x03;
/// `MPI_MSG_SCSI_TASK_TYPE_RESET_BUS`.
pub const MPI_MSG_SCSI_TASK_TYPE_RESET_BUS: u8 = 0x04;
/// `MPI_MSG_SCSI_TASK_TYPE_LOGICAL_UNIT_RESET`.
pub const MPI_MSG_SCSI_TASK_TYPE_LOGICAL_UNIT_RESET: u8 = 0x05;
/// `struct mpi_msg_scsi_task_request`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgScsiTaskRequest {
    /// `target_id`.
    pub target_id: u8,
    /// `bus`.
    pub bus: u8,
    /// `chain_offset`.
    pub chain_offset: u8,
    /// `function`.
    pub function: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `task_type`.
    pub task_type: u8,
    /// `reserved2`.
    pub reserved2: u8,
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
    /// `lun`.
    pub lun: [u16; 4],
    /// `reserved3`: wtf?.
    pub reserved3: [u32; 7],
    /// `target_msg_context`.
    pub target_msg_context: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgScsiTaskRequest>() == 52);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskRequest, target_id) == 0);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskRequest, bus) == 1);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskRequest, chain_offset) == 2);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskRequest, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskRequest, task_type) == 5);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskRequest, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskRequest, msg_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskRequest, lun) == 12);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskRequest, target_msg_context) == 48);
};
/// `struct mpi_msg_scsi_task_reply`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgScsiTaskReply {
    /// `target_id`.
    pub target_id: u8,
    /// `bus`.
    pub bus: u8,
    /// `msg_length`.
    pub msg_length: u8,
    /// `function`.
    pub function: u8,
    /// `response_code`.
    pub response_code: u8,
    /// `task_type`.
    pub task_type: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
    /// `reserved2`.
    pub reserved2: u16,
    /// `ioc_status`.
    pub ioc_status: u16,
    /// `ioc_loginfo`.
    pub ioc_loginfo: u32,
    /// `termination_count`.
    pub termination_count: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgScsiTaskReply>() == 24);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskReply, target_id) == 0);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskReply, bus) == 1);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskReply, msg_length) == 2);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskReply, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskReply, response_code) == 4);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskReply, task_type) == 5);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskReply, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskReply, msg_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskReply, ioc_status) == 14);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskReply, ioc_loginfo) == 16);
    assert!(core::mem::offset_of!(MpiMsgScsiTaskReply, termination_count) == 20);
};
/// `MPI_MSG_RAID_ACTION_STATUS`.
pub const MPI_MSG_RAID_ACTION_STATUS: u8 = 0x00;
/// `MPI_MSG_RAID_ACTION_INDICATOR_STRUCT`.
pub const MPI_MSG_RAID_ACTION_INDICATOR_STRUCT: u8 = 0x01;
/// `MPI_MSG_RAID_ACTION_CREATE_VOLUME`.
pub const MPI_MSG_RAID_ACTION_CREATE_VOLUME: u8 = 0x02;
/// `MPI_MSG_RAID_ACTION_DELETE_VOLUME`.
pub const MPI_MSG_RAID_ACTION_DELETE_VOLUME: u8 = 0x03;
/// `MPI_MSG_RAID_ACTION_DISABLE_VOLUME`.
pub const MPI_MSG_RAID_ACTION_DISABLE_VOLUME: u8 = 0x04;
/// `MPI_MSG_RAID_ACTION_ENABLE_VOLUME`.
pub const MPI_MSG_RAID_ACTION_ENABLE_VOLUME: u8 = 0x05;
/// `MPI_MSG_RAID_ACTION_QUIESCE_PHYSIO`.
pub const MPI_MSG_RAID_ACTION_QUIESCE_PHYSIO: u8 = 0x06;
/// `MPI_MSG_RAID_ACTION_ENABLE_PHYSIO`.
pub const MPI_MSG_RAID_ACTION_ENABLE_PHYSIO: u8 = 0x07;
/// `MPI_MSG_RAID_ACTION_CH_VOL_SETTINGS`.
pub const MPI_MSG_RAID_ACTION_CH_VOL_SETTINGS: u8 = 0x08;
/// `MPI_MSG_RAID_ACTION_PHYSDISK_OFFLINE`.
pub const MPI_MSG_RAID_ACTION_PHYSDISK_OFFLINE: u8 = 0x0a;
/// `MPI_MSG_RAID_ACTION_PHYSDISK_ONLINE`.
pub const MPI_MSG_RAID_ACTION_PHYSDISK_ONLINE: u8 = 0x0b;
/// `MPI_MSG_RAID_ACTION_CH_PHYSDISK_SETTINGS`.
pub const MPI_MSG_RAID_ACTION_CH_PHYSDISK_SETTINGS: u8 = 0x0c;
/// `MPI_MSG_RAID_ACTION_CREATE_PHYSDISK`.
pub const MPI_MSG_RAID_ACTION_CREATE_PHYSDISK: u8 = 0x0d;
/// `MPI_MSG_RAID_ACTION_DELETE_PHYSDISK`.
pub const MPI_MSG_RAID_ACTION_DELETE_PHYSDISK: u8 = 0x0e;
/// `MPI_MSG_RAID_ACTION_PHYSDISK_FAIL`.
pub const MPI_MSG_RAID_ACTION_PHYSDISK_FAIL: u8 = 0x0f;
/// `MPI_MSG_RAID_ACTION_ACTIVATE_VOLUME`.
pub const MPI_MSG_RAID_ACTION_ACTIVATE_VOLUME: u8 = 0x11;
/// `MPI_MSG_RAID_ACTION_DEACTIVATE_VOLUME`.
pub const MPI_MSG_RAID_ACTION_DEACTIVATE_VOLUME: u8 = 0x12;
/// `MPI_MSG_RAID_ACTION_SET_RESYNC_RATE`.
pub const MPI_MSG_RAID_ACTION_SET_RESYNC_RATE: u8 = 0x13;
/// `MPI_MSG_RAID_ACTION_SET_SCRUB_RATE`.
pub const MPI_MSG_RAID_ACTION_SET_SCRUB_RATE: u8 = 0x14;
/// `MPI_MSG_RAID_ACTION_DEVICE_FW_UPDATE_MODE`.
pub const MPI_MSG_RAID_ACTION_DEVICE_FW_UPDATE_MODE: u8 = 0x15;
/// `MPI_MSG_RAID_ACTION_SET_VOL_NAME`.
pub const MPI_MSG_RAID_ACTION_SET_VOL_NAME: u8 = 0x16;
/// `struct mpi_msg_raid_action_request`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgRaidActionRequest {
    /// `action`.
    pub action: u8,
    /// `_reserved1`.
    pub _reserved1: u8,
    /// `chain_offset`.
    pub chain_offset: u8,
    /// `function`.
    pub function: u8,
    /// `vol_id`.
    pub vol_id: u8,
    /// `vol_bus`.
    pub vol_bus: u8,
    /// `phys_disk_num`.
    pub phys_disk_num: u8,
    /// `message_flags`.
    pub message_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
    /// `_reserved2`.
    pub _reserved2: u32,
    /// `data_word`.
    pub data_word: u32,
    /// `data_sge`.
    pub data_sge: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgRaidActionRequest>() == 24);
    assert!(core::mem::offset_of!(MpiMsgRaidActionRequest, action) == 0);
    assert!(core::mem::offset_of!(MpiMsgRaidActionRequest, chain_offset) == 2);
    assert!(core::mem::offset_of!(MpiMsgRaidActionRequest, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgRaidActionRequest, vol_id) == 4);
    assert!(core::mem::offset_of!(MpiMsgRaidActionRequest, vol_bus) == 5);
    assert!(core::mem::offset_of!(MpiMsgRaidActionRequest, phys_disk_num) == 6);
    assert!(core::mem::offset_of!(MpiMsgRaidActionRequest, message_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgRaidActionRequest, msg_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgRaidActionRequest, data_word) == 16);
    assert!(core::mem::offset_of!(MpiMsgRaidActionRequest, data_sge) == 20);
};
/// `MPI_RAID_ACTION_STATUS_OK`.
pub const MPI_RAID_ACTION_STATUS_OK: u16 = 0x0000;
/// `MPI_RAID_ACTION_STATUS_INVALID`.
pub const MPI_RAID_ACTION_STATUS_INVALID: u16 = 0x0001;
/// `MPI_RAID_ACTION_STATUS_FAILURE`.
pub const MPI_RAID_ACTION_STATUS_FAILURE: u16 = 0x0002;
/// `MPI_RAID_ACTION_STATUS_IN_PROGRESS`.
pub const MPI_RAID_ACTION_STATUS_IN_PROGRESS: u16 = 0x0004;
/// `struct mpi_msg_raid_action_reply`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgRaidActionReply {
    /// `action`.
    pub action: u8,
    /// `_reserved1`.
    pub _reserved1: u8,
    /// `message_length`.
    pub message_length: u8,
    /// `function`.
    pub function: u8,
    /// `vol_id`.
    pub vol_id: u8,
    /// `vol_bus`.
    pub vol_bus: u8,
    /// `phys_disk_num`.
    pub phys_disk_num: u8,
    /// `message_flags`.
    pub message_flags: u8,
    /// `message_context`.
    pub message_context: u32,
    /// `action_status`.
    pub action_status: u16,
    /// `ioc_status`.
    pub ioc_status: u16,
    /// `ioc_log_info`.
    pub ioc_log_info: u32,
    /// `volume_status`.
    pub volume_status: u32,
    /// `action_data`.
    pub action_data: u32,
}
const _: () = {
    assert!(size_of::<MpiMsgRaidActionReply>() == 28);
    assert!(core::mem::offset_of!(MpiMsgRaidActionReply, action) == 0);
    assert!(core::mem::offset_of!(MpiMsgRaidActionReply, message_length) == 2);
    assert!(core::mem::offset_of!(MpiMsgRaidActionReply, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgRaidActionReply, vol_id) == 4);
    assert!(core::mem::offset_of!(MpiMsgRaidActionReply, vol_bus) == 5);
    assert!(core::mem::offset_of!(MpiMsgRaidActionReply, phys_disk_num) == 6);
    assert!(core::mem::offset_of!(MpiMsgRaidActionReply, message_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgRaidActionReply, message_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgRaidActionReply, action_status) == 12);
    assert!(core::mem::offset_of!(MpiMsgRaidActionReply, ioc_status) == 14);
    assert!(core::mem::offset_of!(MpiMsgRaidActionReply, ioc_log_info) == 16);
    assert!(core::mem::offset_of!(MpiMsgRaidActionReply, volume_status) == 20);
    assert!(core::mem::offset_of!(MpiMsgRaidActionReply, action_data) == 24);
};
/// `MPI_CONFIG_REQ_PAGE_TYPE_ATTRIBUTE`.
pub const MPI_CONFIG_REQ_PAGE_TYPE_ATTRIBUTE: u8 = 0xf0;
/// `MPI_CONFIG_REQ_PAGE_TYPE_MASK`.
pub const MPI_CONFIG_REQ_PAGE_TYPE_MASK: u8 = 0x0f;
/// `MPI_CONFIG_REQ_PAGE_TYPE_IO_UNIT`.
pub const MPI_CONFIG_REQ_PAGE_TYPE_IO_UNIT: u8 = 0x00;
/// `MPI_CONFIG_REQ_PAGE_TYPE_IOC`.
pub const MPI_CONFIG_REQ_PAGE_TYPE_IOC: u8 = 0x01;
/// `MPI_CONFIG_REQ_PAGE_TYPE_BIOS`.
pub const MPI_CONFIG_REQ_PAGE_TYPE_BIOS: u8 = 0x02;
/// `MPI_CONFIG_REQ_PAGE_TYPE_SCSI_SPI_PORT`.
pub const MPI_CONFIG_REQ_PAGE_TYPE_SCSI_SPI_PORT: u8 = 0x03;
/// `MPI_CONFIG_REQ_PAGE_TYPE_SCSI_SPI_DEV`.
pub const MPI_CONFIG_REQ_PAGE_TYPE_SCSI_SPI_DEV: u8 = 0x04;
/// `MPI_CONFIG_REQ_PAGE_TYPE_FC_PORT`.
pub const MPI_CONFIG_REQ_PAGE_TYPE_FC_PORT: u8 = 0x05;
/// `MPI_CONFIG_REQ_PAGE_TYPE_FC_DEV`.
pub const MPI_CONFIG_REQ_PAGE_TYPE_FC_DEV: u8 = 0x06;
/// `MPI_CONFIG_REQ_PAGE_TYPE_LAN`.
pub const MPI_CONFIG_REQ_PAGE_TYPE_LAN: u8 = 0x07;
/// `MPI_CONFIG_REQ_PAGE_TYPE_RAID_VOL`.
pub const MPI_CONFIG_REQ_PAGE_TYPE_RAID_VOL: u8 = 0x08;
/// `MPI_CONFIG_REQ_PAGE_TYPE_MANUFACTURING`.
pub const MPI_CONFIG_REQ_PAGE_TYPE_MANUFACTURING: u8 = 0x09;
/// `MPI_CONFIG_REQ_PAGE_TYPE_RAID_PD`.
pub const MPI_CONFIG_REQ_PAGE_TYPE_RAID_PD: u8 = 0x0A;
/// `MPI_CONFIG_REQ_PAGE_TYPE_INBAND`.
pub const MPI_CONFIG_REQ_PAGE_TYPE_INBAND: u8 = 0x0B;
/// `MPI_CONFIG_REQ_PAGE_TYPE_EXTENDED`.
pub const MPI_CONFIG_REQ_PAGE_TYPE_EXTENDED: u8 = 0x0F;
/// `struct mpi_cfg_hdr`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgHdr {
    /// `page_version`.
    pub page_version: u8,
    /// `page_length`.
    pub page_length: u8,
    /// `page_number`.
    pub page_number: u8,
    /// `page_type`.
    pub page_type: u8,
}
const _: () = {
    assert!(size_of::<MpiCfgHdr>() == 4);
    assert!(core::mem::offset_of!(MpiCfgHdr, page_version) == 0);
    assert!(core::mem::offset_of!(MpiCfgHdr, page_length) == 1);
    assert!(core::mem::offset_of!(MpiCfgHdr, page_number) == 2);
    assert!(core::mem::offset_of!(MpiCfgHdr, page_type) == 3);
};
/// `struct mpi_ecfg_hdr`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiEcfgHdr {
    /// `page_version`.
    pub page_version: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `page_number`.
    pub page_number: u8,
    /// `page_type`.
    pub page_type: u8,
    /// `ext_page_length`.
    pub ext_page_length: u16,
    /// `ext_page_type`.
    pub ext_page_type: u8,
    /// `reserved2`.
    pub reserved2: u8,
}
const _: () = {
    assert!(size_of::<MpiEcfgHdr>() == 8);
    assert!(core::mem::offset_of!(MpiEcfgHdr, page_version) == 0);
    assert!(core::mem::offset_of!(MpiEcfgHdr, page_number) == 2);
    assert!(core::mem::offset_of!(MpiEcfgHdr, page_type) == 3);
    assert!(core::mem::offset_of!(MpiEcfgHdr, ext_page_length) == 4);
    assert!(core::mem::offset_of!(MpiEcfgHdr, ext_page_type) == 6);
};
/// `MPI_CONFIG_REQ_ACTION_PAGE_HEADER`.
pub const MPI_CONFIG_REQ_ACTION_PAGE_HEADER: u8 = 0x00;
/// `MPI_CONFIG_REQ_ACTION_PAGE_READ_CURRENT`.
pub const MPI_CONFIG_REQ_ACTION_PAGE_READ_CURRENT: u8 = 0x01;
/// `MPI_CONFIG_REQ_ACTION_PAGE_WRITE_CURRENT`.
pub const MPI_CONFIG_REQ_ACTION_PAGE_WRITE_CURRENT: u8 = 0x02;
/// `MPI_CONFIG_REQ_ACTION_PAGE_DEFAULT`.
pub const MPI_CONFIG_REQ_ACTION_PAGE_DEFAULT: u8 = 0x03;
/// `MPI_CONFIG_REQ_ACTION_PAGE_WRITE_NVRAM`.
pub const MPI_CONFIG_REQ_ACTION_PAGE_WRITE_NVRAM: u8 = 0x04;
/// `MPI_CONFIG_REQ_ACTION_PAGE_READ_DEFAULT`.
pub const MPI_CONFIG_REQ_ACTION_PAGE_READ_DEFAULT: u8 = 0x05;
/// `MPI_CONFIG_REQ_ACTION_PAGE_READ_NVRAM`.
pub const MPI_CONFIG_REQ_ACTION_PAGE_READ_NVRAM: u8 = 0x06;
/// `MPI_CONFIG_REQ_EXTPAGE_TYPE_SAS_IO_UNIT`.
pub const MPI_CONFIG_REQ_EXTPAGE_TYPE_SAS_IO_UNIT: u8 = 0x10;
/// `MPI_CONFIG_REQ_EXTPAGE_TYPE_SAS_EXPANDER`.
pub const MPI_CONFIG_REQ_EXTPAGE_TYPE_SAS_EXPANDER: u8 = 0x11;
/// `MPI_CONFIG_REQ_EXTPAGE_TYPE_SAS_DEVICE`.
pub const MPI_CONFIG_REQ_EXTPAGE_TYPE_SAS_DEVICE: u8 = 0x12;
/// `MPI_CONFIG_REQ_EXTPAGE_TYPE_SAS_PHY`.
pub const MPI_CONFIG_REQ_EXTPAGE_TYPE_SAS_PHY: u8 = 0x13;
/// `MPI_CONFIG_REQ_EXTPAGE_TYPE_LOG`.
pub const MPI_CONFIG_REQ_EXTPAGE_TYPE_LOG: u8 = 0x14;
/// `struct mpi_msg_config_request`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgConfigRequest {
    /// `action`.
    pub action: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `chain_offset`.
    pub chain_offset: u8,
    /// `function`.
    pub function: u8,
    /// `ext_page_len`.
    pub ext_page_len: u16,
    /// `ext_page_type`.
    pub ext_page_type: u8,
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
    /// `reserved2`.
    pub reserved2: [u32; 2],
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `page_address`.
    pub page_address: u32,
    /// `page_buffer`.
    pub page_buffer: MpiSge,
}
const _: () = {
    assert!(size_of::<MpiMsgConfigRequest>() == 40);
    assert!(core::mem::offset_of!(MpiMsgConfigRequest, action) == 0);
    assert!(core::mem::offset_of!(MpiMsgConfigRequest, chain_offset) == 2);
    assert!(core::mem::offset_of!(MpiMsgConfigRequest, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgConfigRequest, ext_page_len) == 4);
    assert!(core::mem::offset_of!(MpiMsgConfigRequest, ext_page_type) == 6);
    assert!(core::mem::offset_of!(MpiMsgConfigRequest, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgConfigRequest, msg_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgConfigRequest, config_header) == 20);
    assert!(core::mem::offset_of!(MpiMsgConfigRequest, page_address) == 24);
    assert!(core::mem::offset_of!(MpiMsgConfigRequest, page_buffer) == 28);
};
/// `struct mpi_msg_config_reply`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiMsgConfigReply {
    /// `action`.
    pub action: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `msg_length`.
    pub msg_length: u8,
    /// `function`.
    pub function: u8,
    /// `ext_page_length`.
    pub ext_page_length: u16,
    /// `ext_page_type`.
    pub ext_page_type: u8,
    /// `msg_flags`.
    pub msg_flags: u8,
    /// `msg_context`.
    pub msg_context: u32,
    /// `reserved2`.
    pub reserved2: u16,
    /// `ioc_status`.
    pub ioc_status: u16,
    /// `ioc_loginfo`.
    pub ioc_loginfo: u32,
    /// `config_header`.
    pub config_header: MpiCfgHdr,
}
const _: () = {
    assert!(size_of::<MpiMsgConfigReply>() == 24);
    assert!(core::mem::offset_of!(MpiMsgConfigReply, action) == 0);
    assert!(core::mem::offset_of!(MpiMsgConfigReply, msg_length) == 2);
    assert!(core::mem::offset_of!(MpiMsgConfigReply, function) == 3);
    assert!(core::mem::offset_of!(MpiMsgConfigReply, ext_page_length) == 4);
    assert!(core::mem::offset_of!(MpiMsgConfigReply, ext_page_type) == 6);
    assert!(core::mem::offset_of!(MpiMsgConfigReply, msg_flags) == 7);
    assert!(core::mem::offset_of!(MpiMsgConfigReply, msg_context) == 8);
    assert!(core::mem::offset_of!(MpiMsgConfigReply, ioc_status) == 14);
    assert!(core::mem::offset_of!(MpiMsgConfigReply, ioc_loginfo) == 16);
    assert!(core::mem::offset_of!(MpiMsgConfigReply, config_header) == 20);
};
/// `MPI_CFG_SPI_PORT_0_CAPABILITIES_PACKETIZED`.
pub const MPI_CFG_SPI_PORT_0_CAPABILITIES_PACKETIZED: u8 = 1;
/// `MPI_CFG_SPI_PORT_0_CAPABILITIES_DT`.
pub const MPI_CFG_SPI_PORT_0_CAPABILITIES_DT: u8 = 1 << 1;
/// `MPI_CFG_SPI_PORT_0_CAPABILITIES_QAS`.
pub const MPI_CFG_SPI_PORT_0_CAPABILITIES_QAS: u8 = 1 << 2;
/// `MPI_CFG_SPI_PORT_0_CAPABILITIES_IDP`.
pub const MPI_CFG_SPI_PORT_0_CAPABILITIES_IDP: u8 = 1 << 3;
/// `MPI_CFG_SPI_PORT_0_CAPABILITIES_WIDTH`.
pub const MPI_CFG_SPI_PORT_0_CAPABILITIES_WIDTH: u8 = 1 << 5;
/// `MPI_CFG_SPI_PORT_0_CAPABILITIES_WIDTH_NARROW`.
pub const MPI_CFG_SPI_PORT_0_CAPABILITIES_WIDTH_NARROW: u8 = 0;
/// `MPI_CFG_SPI_PORT_0_CAPABILITIES_WIDTH_WIDE`.
pub const MPI_CFG_SPI_PORT_0_CAPABILITIES_WIDTH_WIDE: u8 = 1 << 5;
/// `MPI_CFG_SPI_PORT_0_CAPABILITIES_AIP`.
pub const MPI_CFG_SPI_PORT_0_CAPABILITIES_AIP: u8 = 1 << 7;
/// `MPI_CFG_SPI_PORT_0_SIGNAL_HVD`.
pub const MPI_CFG_SPI_PORT_0_SIGNAL_HVD: u8 = 0x1;
/// `MPI_CFG_SPI_PORT_0_SIGNAL_SE`.
pub const MPI_CFG_SPI_PORT_0_SIGNAL_SE: u8 = 0x2;
/// `MPI_CFG_SPI_PORT_0_SIGNAL_LVD`.
pub const MPI_CFG_SPI_PORT_0_SIGNAL_LVD: u8 = 0x3;
/// `MPI_CFG_SPI_PORT_0_CONNECTEDID_BUSFREE`.
pub const MPI_CFG_SPI_PORT_0_CONNECTEDID_BUSFREE: u8 = 0xfe;
/// `MPI_CFG_SPI_PORT_0_CONNECTEDID_UNKNOWN`.
pub const MPI_CFG_SPI_PORT_0_CONNECTEDID_UNKNOWN: u8 = 0xff;
/// `struct mpi_cfg_spi_port_pg0`.
#[derive(Clone, Copy, Default)]
#[repr(C, packed)]
pub struct MpiCfgSpiPortPg0 {
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `capabilities1`.
    pub capabilities1: u8,
    /// `min_period`.
    pub min_period: u8,
    /// `max_offset`.
    pub max_offset: u8,
    /// `capabilities2`.
    pub capabilities2: u8,
    /// `signalling_type`.
    pub signalling_type: u8,
    /// `reserved`.
    pub reserved: u16,
    /// `connected_id`.
    pub connected_id: u8,
}
const _: () = {
    assert!(size_of::<MpiCfgSpiPortPg0>() == 12);
    assert!(core::mem::offset_of!(MpiCfgSpiPortPg0, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgSpiPortPg0, capabilities1) == 4);
    assert!(core::mem::offset_of!(MpiCfgSpiPortPg0, min_period) == 5);
    assert!(core::mem::offset_of!(MpiCfgSpiPortPg0, max_offset) == 6);
    assert!(core::mem::offset_of!(MpiCfgSpiPortPg0, capabilities2) == 7);
    assert!(core::mem::offset_of!(MpiCfgSpiPortPg0, signalling_type) == 8);
    assert!(core::mem::offset_of!(MpiCfgSpiPortPg0, connected_id) == 11);
};
/// `MPI_CFG_SPI_PORT_1_TARGCFG_TARGET_ONLY`.
pub const MPI_CFG_SPI_PORT_1_TARGCFG_TARGET_ONLY: u8 = 0x01;
/// `MPI_CFG_SPI_PORT_1_TARGCFG_INIT_TARGET`.
pub const MPI_CFG_SPI_PORT_1_TARGCFG_INIT_TARGET: u8 = 0x02;
/// `struct mpi_cfg_spi_port_pg1`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgSpiPortPg1 {
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `port_scsi_id`.
    pub port_scsi_id: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `port_resp_ids`.
    pub port_resp_ids: u16,
    /// `on_bus_timer_value`.
    pub on_bus_timer_value: u32,
    /// `target_config`.
    pub target_config: u8,
    /// `reserved2`.
    pub reserved2: u8,
    /// `id_config`.
    pub id_config: u16,
}
const _: () = {
    assert!(size_of::<MpiCfgSpiPortPg1>() == 16);
    assert!(core::mem::offset_of!(MpiCfgSpiPortPg1, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgSpiPortPg1, port_scsi_id) == 4);
    assert!(core::mem::offset_of!(MpiCfgSpiPortPg1, port_resp_ids) == 6);
    assert!(core::mem::offset_of!(MpiCfgSpiPortPg1, on_bus_timer_value) == 8);
    assert!(core::mem::offset_of!(MpiCfgSpiPortPg1, target_config) == 12);
    assert!(core::mem::offset_of!(MpiCfgSpiPortPg1, id_config) == 14);
};
/// `MPI_CFG_SPI_PORT_2_DEV_FLAG_DISCONNECT_EN`.
pub const MPI_CFG_SPI_PORT_2_DEV_FLAG_DISCONNECT_EN: u16 = 1;
/// `MPI_CFG_SPI_PORT_2_DEV_FLAG_SCAN_ID_EN`.
pub const MPI_CFG_SPI_PORT_2_DEV_FLAG_SCAN_ID_EN: u16 = 1 << 1;
/// `MPI_CFG_SPI_PORT_2_DEV_FLAG_SCAN_LUN_EN`.
pub const MPI_CFG_SPI_PORT_2_DEV_FLAG_SCAN_LUN_EN: u16 = 1 << 2;
/// `MPI_CFG_SPI_PORT_2_DEV_FLAG_TAQ_Q_EN`.
pub const MPI_CFG_SPI_PORT_2_DEV_FLAG_TAQ_Q_EN: u16 = 1 << 3;
/// `MPI_CFG_SPI_PORT_2_DEV_FLAG_WIDE_DIS`.
pub const MPI_CFG_SPI_PORT_2_DEV_FLAG_WIDE_DIS: u16 = 1 << 4;
/// `MPI_CFG_SPI_PORT_2_DEV_FLAG_BOOT_CHOICE`.
pub const MPI_CFG_SPI_PORT_2_DEV_FLAG_BOOT_CHOICE: u16 = 1 << 5;
/// The anonymous struct of `device_settings[]` in `struct mpi_cfg_spi_port_pg2`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgSpiPortPg2DeviceSettings {
    /// `timeout`.
    pub timeout: u8,
    /// `sync_factor`.
    pub sync_factor: u8,
    /// `device_flags`.
    pub device_flags: u16,
}
const _: () = assert!(size_of::<MpiCfgSpiPortPg2DeviceSettings>() == 4);
/// `MPI_CFG_SPI_PORT_2_PORT_FLAGS_SCAN_HI2LOW`.
pub const MPI_CFG_SPI_PORT_2_PORT_FLAGS_SCAN_HI2LOW: u32 = 1;
/// `MPI_CFG_SPI_PORT_2_PORT_FLAGS_AVOID_RESET`.
pub const MPI_CFG_SPI_PORT_2_PORT_FLAGS_AVOID_RESET: u32 = 1 << 2;
/// `MPI_CFG_SPI_PORT_2_PORT_FLAGS_ALT_CHS`.
pub const MPI_CFG_SPI_PORT_2_PORT_FLAGS_ALT_CHS: u32 = 1 << 3;
/// `MPI_CFG_SPI_PORT_2_PORT_FLAGS_TERM_DISABLED`.
pub const MPI_CFG_SPI_PORT_2_PORT_FLAGS_TERM_DISABLED: u32 = 1 << 4;
/// `MPI_CFG_SPI_PORT_2_PORT_FLAGS_DV_CTL`.
pub const MPI_CFG_SPI_PORT_2_PORT_FLAGS_DV_CTL: u32 = 0x3 << 5;
/// `MPI_CFG_SPI_PORT_2_PORT_FLAGS_DV_HOST_BE`.
pub const MPI_CFG_SPI_PORT_2_PORT_FLAGS_DV_HOST_BE: u32 = 0;
/// `MPI_CFG_SPI_PORT_2_PORT_FLAGS_DV_HOST_B`.
pub const MPI_CFG_SPI_PORT_2_PORT_FLAGS_DV_HOST_B: u32 = 0x1 << 5;
/// `MPI_CFG_SPI_PORT_2_PORT_FLAGS_DV_HOST_NONE`.
pub const MPI_CFG_SPI_PORT_2_PORT_FLAGS_DV_HOST_NONE: u32 = 0x3 << 5;
/// `MPI_CFG_SPI_PORT_2_PORT_SET_HOST_ID`.
pub const MPI_CFG_SPI_PORT_2_PORT_SET_HOST_ID: u32 = 0x7;
/// `MPI_CFG_SPI_PORT_2_PORT_SET_INIT_HBA`.
pub const MPI_CFG_SPI_PORT_2_PORT_SET_INIT_HBA: u32 = 0x3 << 4;
/// `MPI_CFG_SPI_PORT_2_PORT_SET_INIT_HBA_DISABLED`.
pub const MPI_CFG_SPI_PORT_2_PORT_SET_INIT_HBA_DISABLED: u32 = 0;
/// `MPI_CFG_SPI_PORT_2_PORT_SET_INIT_HBA_BIOS`.
pub const MPI_CFG_SPI_PORT_2_PORT_SET_INIT_HBA_BIOS: u32 = 0x1 << 4;
/// `MPI_CFG_SPI_PORT_2_PORT_SET_INIT_HBA_OS`.
pub const MPI_CFG_SPI_PORT_2_PORT_SET_INIT_HBA_OS: u32 = 0x2 << 4;
/// `MPI_CFG_SPI_PORT_2_PORT_SET_INIT_HBA_BIOS_OS`.
pub const MPI_CFG_SPI_PORT_2_PORT_SET_INIT_HBA_BIOS_OS: u32 = 0x3 << 4;
/// `MPI_CFG_SPI_PORT_2_PORT_SET_REMOVABLE`.
pub const MPI_CFG_SPI_PORT_2_PORT_SET_REMOVABLE: u32 = 0x3 << 6;
/// `MPI_CFG_SPI_PORT_2_PORT_SET_SPINUP_DELAY`.
pub const MPI_CFG_SPI_PORT_2_PORT_SET_SPINUP_DELAY: u32 = 0xf << 8;
/// `MPI_CFG_SPI_PORT_2_PORT_SET_SYNC`.
pub const MPI_CFG_SPI_PORT_2_PORT_SET_SYNC: u32 = 0x3 << 12;
/// `MPI_CFG_SPI_PORT_2_PORT_SET_NEG_SUPPORTED`.
pub const MPI_CFG_SPI_PORT_2_PORT_SET_NEG_SUPPORTED: u32 = 0;
/// `MPI_CFG_SPI_PORT_2_PORT_SET_NEG_NONE`.
pub const MPI_CFG_SPI_PORT_2_PORT_SET_NEG_NONE: u32 = 0x1 << 12;
/// `MPI_CFG_SPI_PORT_2_PORT_SET_NEG_ALL`.
pub const MPI_CFG_SPI_PORT_2_PORT_SET_NEG_ALL: u32 = 0x3 << 12;
/// `struct mpi_cfg_spi_port_pg2`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgSpiPortPg2 {
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `port_flags`.
    pub port_flags: u32,
    /// `port_settings`.
    pub port_settings: u32,
    /// `device_settings`.
    pub device_settings: [MpiCfgSpiPortPg2DeviceSettings; 16],
}
const _: () = {
    assert!(size_of::<MpiCfgSpiPortPg2>() == 76);
    assert!(core::mem::offset_of!(MpiCfgSpiPortPg2, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgSpiPortPg2, port_flags) == 4);
    assert!(core::mem::offset_of!(MpiCfgSpiPortPg2, port_settings) == 8);
    assert!(core::mem::offset_of!(MpiCfgSpiPortPg2, device_settings) == 12);
};
/// `MPI_CFG_SPI_DEV_0_NEGPARAMS_PACKETIZED`.
pub const MPI_CFG_SPI_DEV_0_NEGPARAMS_PACKETIZED: u8 = 1;
/// `MPI_CFG_SPI_DEV_0_NEGPARAMS_DUALXFERS`.
pub const MPI_CFG_SPI_DEV_0_NEGPARAMS_DUALXFERS: u8 = 1 << 1;
/// `MPI_CFG_SPI_DEV_0_NEGPARAMS_QAS`.
pub const MPI_CFG_SPI_DEV_0_NEGPARAMS_QAS: u8 = 1 << 2;
/// `MPI_CFG_SPI_DEV_0_NEGPARAMS_HOLD_MCS`.
pub const MPI_CFG_SPI_DEV_0_NEGPARAMS_HOLD_MCS: u8 = 1 << 3;
/// `MPI_CFG_SPI_DEV_0_NEGPARAMS_WR_FLOW`.
pub const MPI_CFG_SPI_DEV_0_NEGPARAMS_WR_FLOW: u8 = 1 << 4;
/// `MPI_CFG_SPI_DEV_0_NEGPARAMS_RD_STRM`.
pub const MPI_CFG_SPI_DEV_0_NEGPARAMS_RD_STRM: u8 = 1 << 5;
/// `MPI_CFG_SPI_DEV_0_NEGPARAMS_RTI`.
pub const MPI_CFG_SPI_DEV_0_NEGPARAMS_RTI: u8 = 1 << 6;
/// `MPI_CFG_SPI_DEV_0_NEGPARAMS_PCOMP_EN`.
pub const MPI_CFG_SPI_DEV_0_NEGPARAMS_PCOMP_EN: u8 = 1 << 7;
/// `MPI_CFG_SPI_DEV_0_NEGPARAMS_IDP_EN`.
pub const MPI_CFG_SPI_DEV_0_NEGPARAMS_IDP_EN: u8 = 1 << 3;
/// `MPI_CFG_SPI_DEV_0_NEGPARAMS_WIDTH`.
pub const MPI_CFG_SPI_DEV_0_NEGPARAMS_WIDTH: u8 = 1 << 5;
/// `MPI_CFG_SPI_DEV_0_NEGPARAMS_WIDTH_NARROW`.
pub const MPI_CFG_SPI_DEV_0_NEGPARAMS_WIDTH_NARROW: u8 = 0;
/// `MPI_CFG_SPI_DEV_0_NEGPARAMS_WIDTH_WIDE`.
pub const MPI_CFG_SPI_DEV_0_NEGPARAMS_WIDTH_WIDE: u8 = 1 << 5;
/// `MPI_CFG_SPI_DEV_0_NEGPARAMS_AIP`.
pub const MPI_CFG_SPI_DEV_0_NEGPARAMS_AIP: u8 = 1 << 7;
/// `MPI_CFG_SPI_DEV_0_INFO_NEG_OCCURRED`.
pub const MPI_CFG_SPI_DEV_0_INFO_NEG_OCCURRED: u32 = 1;
/// `MPI_CFG_SPI_DEV_0_INFO_SDTR_REJECTED`.
pub const MPI_CFG_SPI_DEV_0_INFO_SDTR_REJECTED: u32 = 1 << 1;
/// `MPI_CFG_SPI_DEV_0_INFO_WDTR_REJECTED`.
pub const MPI_CFG_SPI_DEV_0_INFO_WDTR_REJECTED: u32 = 1 << 2;
/// `MPI_CFG_SPI_DEV_0_INFO_PPR_REJECTED`.
pub const MPI_CFG_SPI_DEV_0_INFO_PPR_REJECTED: u32 = 1 << 3;
/// `struct mpi_cfg_spi_dev_pg0`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgSpiDevPg0 {
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `neg_params1`.
    pub neg_params1: u8,
    /// `neg_period`.
    pub neg_period: u8,
    /// `neg_offset`.
    pub neg_offset: u8,
    /// `neg_params2`.
    pub neg_params2: u8,
    /// `information`.
    pub information: u32,
}
const _: () = {
    assert!(size_of::<MpiCfgSpiDevPg0>() == 12);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg0, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg0, neg_params1) == 4);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg0, neg_period) == 5);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg0, neg_offset) == 6);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg0, neg_params2) == 7);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg0, information) == 8);
};
/// `MPI_CFG_SPI_DEV_1_REQPARAMS_PACKETIZED`.
pub const MPI_CFG_SPI_DEV_1_REQPARAMS_PACKETIZED: u8 = 1;
/// `MPI_CFG_SPI_DEV_1_REQPARAMS_DUALXFERS`.
pub const MPI_CFG_SPI_DEV_1_REQPARAMS_DUALXFERS: u8 = 1 << 1;
/// `MPI_CFG_SPI_DEV_1_REQPARAMS_QAS`.
pub const MPI_CFG_SPI_DEV_1_REQPARAMS_QAS: u8 = 1 << 2;
/// `MPI_CFG_SPI_DEV_1_REQPARAMS_HOLD_MCS`.
pub const MPI_CFG_SPI_DEV_1_REQPARAMS_HOLD_MCS: u8 = 1 << 3;
/// `MPI_CFG_SPI_DEV_1_REQPARAMS_WR_FLOW`.
pub const MPI_CFG_SPI_DEV_1_REQPARAMS_WR_FLOW: u8 = 1 << 4;
/// `MPI_CFG_SPI_DEV_1_REQPARAMS_RD_STRM`.
pub const MPI_CFG_SPI_DEV_1_REQPARAMS_RD_STRM: u8 = 1 << 5;
/// `MPI_CFG_SPI_DEV_1_REQPARAMS_RTI`.
pub const MPI_CFG_SPI_DEV_1_REQPARAMS_RTI: u8 = 1 << 6;
/// `MPI_CFG_SPI_DEV_1_REQPARAMS_PCOMP_EN`.
pub const MPI_CFG_SPI_DEV_1_REQPARAMS_PCOMP_EN: u8 = 1 << 7;
/// `MPI_CFG_SPI_DEV_1_REQPARAMS_IDP_EN`.
pub const MPI_CFG_SPI_DEV_1_REQPARAMS_IDP_EN: u8 = 1 << 3;
/// `MPI_CFG_SPI_DEV_1_REQPARAMS_WIDTH`.
pub const MPI_CFG_SPI_DEV_1_REQPARAMS_WIDTH: u8 = 1 << 5;
/// `MPI_CFG_SPI_DEV_1_REQPARAMS_WIDTH_NARROW`.
pub const MPI_CFG_SPI_DEV_1_REQPARAMS_WIDTH_NARROW: u8 = 0;
/// `MPI_CFG_SPI_DEV_1_REQPARAMS_WIDTH_WIDE`.
pub const MPI_CFG_SPI_DEV_1_REQPARAMS_WIDTH_WIDE: u8 = 1 << 5;
/// `MPI_CFG_SPI_DEV_1_REQPARAMS_AIP`.
pub const MPI_CFG_SPI_DEV_1_REQPARAMS_AIP: u8 = 1 << 7;
/// `MPI_CFG_SPI_DEV_1_CONF_WDTR_DISALLOWED`.
pub const MPI_CFG_SPI_DEV_1_CONF_WDTR_DISALLOWED: u32 = 1 << 1;
/// `MPI_CFG_SPI_DEV_1_CONF_SDTR_DISALLOWED`.
pub const MPI_CFG_SPI_DEV_1_CONF_SDTR_DISALLOWED: u32 = 1 << 2;
/// `MPI_CFG_SPI_DEV_1_CONF_EXTPARAMS`.
pub const MPI_CFG_SPI_DEV_1_CONF_EXTPARAMS: u32 = 1 << 3;
/// `MPI_CFG_SPI_DEV_1_CONF_FORCE_PPR`.
pub const MPI_CFG_SPI_DEV_1_CONF_FORCE_PPR: u32 = 1 << 4;
/// `struct mpi_cfg_spi_dev_pg1`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgSpiDevPg1 {
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `req_params1`.
    pub req_params1: u8,
    /// `req_period`.
    pub req_period: u8,
    /// `req_offset`.
    pub req_offset: u8,
    /// `req_params2`.
    pub req_params2: u8,
    /// `reserved`.
    pub reserved: u32,
    /// `configuration`.
    pub configuration: u32,
}
const _: () = {
    assert!(size_of::<MpiCfgSpiDevPg1>() == 16);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg1, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg1, req_params1) == 4);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg1, req_period) == 5);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg1, req_offset) == 6);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg1, req_params2) == 7);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg1, configuration) == 12);
};
/// `MPI_CFG_SPI_DEV_2_DV_ISI_ENABLED`.
pub const MPI_CFG_SPI_DEV_2_DV_ISI_ENABLED: u32 = 1 << 4;
/// `MPI_CFG_SPI_DEV_2_DV_SECONDARY_DRV_EN`.
pub const MPI_CFG_SPI_DEV_2_DV_SECONDARY_DRV_EN: u32 = 1 << 5;
/// `MPI_CFG_SPI_DEV_2_DV_SLEW_RATE_CTL`.
pub const MPI_CFG_SPI_DEV_2_DV_SLEW_RATE_CTL: u32 = 0x7 << 7;
/// `MPI_CFG_SPI_DEV_2_DV_PRIMARY_DRV_STRENGTH`.
pub const MPI_CFG_SPI_DEV_2_DV_PRIMARY_DRV_STRENGTH: u32 = 0x7 << 10;
/// `MPI_CFG_SPI_DEV_2_DV_XCLKH_ST`.
pub const MPI_CFG_SPI_DEV_2_DV_XCLKH_ST: u32 = 1 << 28;
/// `MPI_CFG_SPI_DEV_2_DV_XCLKS_ST`.
pub const MPI_CFG_SPI_DEV_2_DV_XCLKS_ST: u32 = 1 << 29;
/// `MPI_CFG_SPI_DEV_2_DV_XCLKH_DT`.
pub const MPI_CFG_SPI_DEV_2_DV_XCLKH_DT: u32 = 1 << 30;
/// `MPI_CFG_SPI_DEV_2_DV_XCLKS_DT`.
pub const MPI_CFG_SPI_DEV_2_DV_XCLKS_DT: u32 = 1 << 31;
/// `MPI_CFG_SPI_DEV_2_PARITY_PIPE_SELECT`.
pub const MPI_CFG_SPI_DEV_2_PARITY_PIPE_SELECT: u32 = 0x3;
/// `MPI_CFG_SPI_DEV_2_DATA_PIPE_SELECT(x)`.
pub const fn mpi_cfg_spi_dev_2_data_pipe_select(x: u32) -> u32 {
    0x3 << (x * 2)
}
/// `struct mpi_cfg_spi_dev_pg2`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgSpiDevPg2 {
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `domain_validation`.
    pub domain_validation: u32,
    /// `parity_pipe_select`.
    pub parity_pipe_select: u32,
    /// `data_pipe_select`.
    pub data_pipe_select: u32,
}
const _: () = {
    assert!(size_of::<MpiCfgSpiDevPg2>() == 16);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg2, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg2, domain_validation) == 4);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg2, parity_pipe_select) == 8);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg2, data_pipe_select) == 12);
};
/// `struct mpi_cfg_spi_dev_pg3`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgSpiDevPg3 {
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `msg_reject_count`.
    pub msg_reject_count: u16,
    /// `phase_error_count`.
    pub phase_error_count: u16,
    /// `parity_error_count`.
    pub parity_error_count: u16,
    /// `reserved`.
    pub reserved: u16,
}
const _: () = {
    assert!(size_of::<MpiCfgSpiDevPg3>() == 12);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg3, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg3, msg_reject_count) == 4);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg3, phase_error_count) == 6);
    assert!(core::mem::offset_of!(MpiCfgSpiDevPg3, parity_error_count) == 8);
};
/// `struct mpi_cfg_manufacturing_pg0`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgManufacturingPg0 {
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `chip_name`.
    pub chip_name: [u8; 16],
    /// `chip_revision`.
    pub chip_revision: [u8; 8],
    /// `board_name`.
    pub board_name: [u8; 16],
    /// `board_assembly`.
    pub board_assembly: [u8; 16],
    /// `board_tracer_number`.
    pub board_tracer_number: [u8; 16],
}
const _: () = {
    assert!(size_of::<MpiCfgManufacturingPg0>() == 76);
    assert!(core::mem::offset_of!(MpiCfgManufacturingPg0, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgManufacturingPg0, chip_name) == 4);
    assert!(core::mem::offset_of!(MpiCfgManufacturingPg0, chip_revision) == 20);
    assert!(core::mem::offset_of!(MpiCfgManufacturingPg0, board_name) == 28);
    assert!(core::mem::offset_of!(MpiCfgManufacturingPg0, board_assembly) == 44);
    assert!(core::mem::offset_of!(MpiCfgManufacturingPg0, board_tracer_number) == 60);
};
/// `MPI_CFG_IOC_1_REPLY_COALESCING`.
pub const MPI_CFG_IOC_1_REPLY_COALESCING: u32 = 1;
/// `MPI_CFG_IOC_1_CTX_REPLY_DISABLE`.
pub const MPI_CFG_IOC_1_CTX_REPLY_DISABLE: u32 = 1 << 4;
/// `struct mpi_cfg_ioc_pg1`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgIocPg1 {
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `flags`.
    pub flags: u32,
    /// `coalescing_timeout`.
    pub coalescing_timeout: u32,
    /// `coalescing_depth`.
    pub coalescing_depth: u8,
    /// `pci_slot_num`.
    pub pci_slot_num: u8,
    /// `_reserved`.
    pub _reserved: [u8; 2],
}
const _: () = {
    assert!(size_of::<MpiCfgIocPg1>() == 16);
    assert!(core::mem::offset_of!(MpiCfgIocPg1, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgIocPg1, flags) == 4);
    assert!(core::mem::offset_of!(MpiCfgIocPg1, coalescing_timeout) == 8);
    assert!(core::mem::offset_of!(MpiCfgIocPg1, coalescing_depth) == 12);
    assert!(core::mem::offset_of!(MpiCfgIocPg1, pci_slot_num) == 13);
};
/// `MPI_CFG_IOC_2_CAPABILITIES_IS`.
pub const MPI_CFG_IOC_2_CAPABILITIES_IS: u32 = 1;
/// `MPI_CFG_IOC_2_CAPABILITIES_IME`.
pub const MPI_CFG_IOC_2_CAPABILITIES_IME: u32 = 1 << 1;
/// `MPI_CFG_IOC_2_CAPABILITIES_IM`.
pub const MPI_CFG_IOC_2_CAPABILITIES_IM: u32 = 1 << 2;
/// `MPI_CFG_IOC_2_CAPABILITIES_RAID`.
pub const MPI_CFG_IOC_2_CAPABILITIES_RAID: u32 =
    MPI_CFG_IOC_2_CAPABILITIES_IS | MPI_CFG_IOC_2_CAPABILITIES_IME | MPI_CFG_IOC_2_CAPABILITIES_IM;
/// `MPI_CFG_IOC_2_CAPABILITIES_SES`.
pub const MPI_CFG_IOC_2_CAPABILITIES_SES: u32 = 1 << 29;
/// `MPI_CFG_IOC_2_CAPABILITIES_SAFTE`.
pub const MPI_CFG_IOC_2_CAPABILITIES_SAFTE: u32 = 1 << 30;
/// `MPI_CFG_IOC_2_CAPABILITIES_XCHANNEL`.
pub const MPI_CFG_IOC_2_CAPABILITIES_XCHANNEL: u32 = 1 << 31;
/// `struct mpi_cfg_ioc_pg2`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgIocPg2 {
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `capabilities`.
    pub capabilities: u32,
    /// `active_vols`.
    pub active_vols: u8,
    /// `max_vols`.
    pub max_vols: u8,
    /// `active_physdisks`.
    pub active_physdisks: u8,
    /// `max_physdisks`.
    pub max_physdisks: u8,
}
const _: () = {
    assert!(size_of::<MpiCfgIocPg2>() == 12);
    assert!(core::mem::offset_of!(MpiCfgIocPg2, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgIocPg2, capabilities) == 4);
    assert!(core::mem::offset_of!(MpiCfgIocPg2, active_vols) == 8);
    assert!(core::mem::offset_of!(MpiCfgIocPg2, max_vols) == 9);
    assert!(core::mem::offset_of!(MpiCfgIocPg2, active_physdisks) == 10);
    assert!(core::mem::offset_of!(MpiCfgIocPg2, max_physdisks) == 11);
};
/// `MPI_CFG_RAID_TYPE_RAID_IS`.
pub const MPI_CFG_RAID_TYPE_RAID_IS: u8 = 0x00;
/// `MPI_CFG_RAID_TYPE_RAID_IME`.
pub const MPI_CFG_RAID_TYPE_RAID_IME: u8 = 0x01;
/// `MPI_CFG_RAID_TYPE_RAID_IM`.
pub const MPI_CFG_RAID_TYPE_RAID_IM: u8 = 0x02;
/// `MPI_CFG_RAID_TYPE_RAID_5`.
pub const MPI_CFG_RAID_TYPE_RAID_5: u8 = 0x03;
/// `MPI_CFG_RAID_TYPE_RAID_6`.
pub const MPI_CFG_RAID_TYPE_RAID_6: u8 = 0x04;
/// `MPI_CFG_RAID_TYPE_RAID_10`.
pub const MPI_CFG_RAID_TYPE_RAID_10: u8 = 0x05;
/// `MPI_CFG_RAID_TYPE_RAID_50`.
pub const MPI_CFG_RAID_TYPE_RAID_50: u8 = 0x06;
/// `MPI_CFG_RAID_VOL_INACTIVE`.
pub const MPI_CFG_RAID_VOL_INACTIVE: u8 = 1 << 3;
/// `struct mpi_cfg_raid_vol`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgRaidVol {
    /// `vol_id`.
    pub vol_id: u8,
    /// `vol_bus`.
    pub vol_bus: u8,
    /// `vol_ioc`.
    pub vol_ioc: u8,
    /// `vol_page`.
    pub vol_page: u8,
    /// `vol_type`.
    pub vol_type: u8,
    /// `flags`.
    pub flags: u8,
    /// `reserved`.
    pub reserved: u16,
}
const _: () = {
    assert!(size_of::<MpiCfgRaidVol>() == 8);
    assert!(core::mem::offset_of!(MpiCfgRaidVol, vol_id) == 0);
    assert!(core::mem::offset_of!(MpiCfgRaidVol, vol_bus) == 1);
    assert!(core::mem::offset_of!(MpiCfgRaidVol, vol_ioc) == 2);
    assert!(core::mem::offset_of!(MpiCfgRaidVol, vol_page) == 3);
    assert!(core::mem::offset_of!(MpiCfgRaidVol, vol_type) == 4);
    assert!(core::mem::offset_of!(MpiCfgRaidVol, flags) == 5);
};
/// `struct mpi_cfg_ioc_pg3`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgIocPg3 {
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `no_phys_disks`.
    pub no_phys_disks: u8,
    /// `reserved`.
    pub reserved: [u8; 3],
}
const _: () = {
    assert!(size_of::<MpiCfgIocPg3>() == 8);
    assert!(core::mem::offset_of!(MpiCfgIocPg3, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgIocPg3, no_phys_disks) == 4);
};
/// `struct mpi_cfg_raid_physdisk`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgRaidPhysdisk {
    /// `phys_disk_id`.
    pub phys_disk_id: u8,
    /// `phys_disk_bus`.
    pub phys_disk_bus: u8,
    /// `phys_disk_ioc`.
    pub phys_disk_ioc: u8,
    /// `phys_disk_num`.
    pub phys_disk_num: u8,
}
const _: () = {
    assert!(size_of::<MpiCfgRaidPhysdisk>() == 4);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdisk, phys_disk_id) == 0);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdisk, phys_disk_bus) == 1);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdisk, phys_disk_ioc) == 2);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdisk, phys_disk_num) == 3);
};
/// `struct mpi_cfg_fc_port_pg0`.
#[derive(Clone, Copy, Default)]
#[repr(C, packed)]
pub struct MpiCfgFcPortPg0 {
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `flags`.
    pub flags: u32,
    /// `mpi_port_nr`.
    pub mpi_port_nr: u8,
    /// `link_type`.
    pub link_type: u8,
    /// `port_state`.
    pub port_state: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `port_id`.
    pub port_id: u32,
    /// `wwnn`.
    pub wwnn: u64,
    /// `wwpn`.
    pub wwpn: u64,
    /// `supported_service_class`.
    pub supported_service_class: u32,
    /// `supported_speeds`.
    pub supported_speeds: u32,
    /// `current_speed`.
    pub current_speed: u32,
    /// `max_frame_size`.
    pub max_frame_size: u32,
    /// `fabric_wwnn`.
    pub fabric_wwnn: u64,
    /// `fabric_wwpn`.
    pub fabric_wwpn: u64,
    /// `discovered_port_count`.
    pub discovered_port_count: u32,
    /// `max_initiators`.
    pub max_initiators: u32,
    /// `max_aliases_supported`.
    pub max_aliases_supported: u8,
    /// `max_hard_aliases_supported`.
    pub max_hard_aliases_supported: u8,
    /// `num_current_aliases`.
    pub num_current_aliases: u8,
    /// `reserved2`.
    pub reserved2: u8,
}
const _: () = {
    assert!(size_of::<MpiCfgFcPortPg0>() == 76);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, flags) == 4);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, mpi_port_nr) == 8);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, link_type) == 9);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, port_state) == 10);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, port_id) == 12);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, wwnn) == 16);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, wwpn) == 24);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, supported_service_class) == 32);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, supported_speeds) == 36);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, current_speed) == 40);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, max_frame_size) == 44);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, fabric_wwnn) == 48);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, fabric_wwpn) == 56);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, discovered_port_count) == 64);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, max_initiators) == 68);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, max_aliases_supported) == 72);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, max_hard_aliases_supported) == 73);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg0, num_current_aliases) == 74);
};
/// `MPI_CFG_FC_PORT_0_FLAGS_MAP_BY_D_ID`.
pub const MPI_CFG_FC_PORT_0_FLAGS_MAP_BY_D_ID: u32 = 1;
/// `MPI_CFG_FC_PORT_0_FLAGS_MAINTAIN_LOGINS`.
pub const MPI_CFG_FC_PORT_0_FLAGS_MAINTAIN_LOGINS: u32 = 1 << 1;
/// `MPI_CFG_FC_PORT_0_FLAGS_PLOGI_AFTER_LOGO`.
pub const MPI_CFG_FC_PORT_0_FLAGS_PLOGI_AFTER_LOGO: u32 = 1 << 2;
/// `MPI_CFG_FC_PORT_0_FLAGS_SUPPRESS_PROT_REG`.
pub const MPI_CFG_FC_PORT_0_FLAGS_SUPPRESS_PROT_REG: u32 = 1 << 3;
/// `MPI_CFG_FC_PORT_0_FLAGS_MASK_RR_TOV_UNITS`.
pub const MPI_CFG_FC_PORT_0_FLAGS_MASK_RR_TOV_UNITS: u32 = 0x7 << 4;
/// `MPI_CFG_FC_PORT_0_FLAGS_MASK_RR_TOV_UNIT_NONE`.
pub const MPI_CFG_FC_PORT_0_FLAGS_MASK_RR_TOV_UNIT_NONE: u32 = 0;
/// `MPI_CFG_FC_PORT_0_FLAGS_MASK_RR_TOV_UNIT_0_001_SEC`.
pub const MPI_CFG_FC_PORT_0_FLAGS_MASK_RR_TOV_UNIT_0_001_SEC: u32 = 0x1 << 4;
/// `MPI_CFG_FC_PORT_0_FLAGS_MASK_RR_TOV_UNIT_0_1_SEC`.
pub const MPI_CFG_FC_PORT_0_FLAGS_MASK_RR_TOV_UNIT_0_1_SEC: u32 = 0x3 << 4;
/// `MPI_CFG_FC_PORT_0_FLAGS_MASK_RR_TOV_UNIT_10_SEC`.
pub const MPI_CFG_FC_PORT_0_FLAGS_MASK_RR_TOV_UNIT_10_SEC: u32 = 0x5 << 4;
/// `MPI_CFG_FC_PORT_0_FLAGS_TGT_LARGE_CDB_EN`.
pub const MPI_CFG_FC_PORT_0_FLAGS_TGT_LARGE_CDB_EN: u32 = 1 << 7;
/// `MPI_CFG_FC_PORT_0_FLAGS_SOFT_ALPA_FALLBACK`.
pub const MPI_CFG_FC_PORT_0_FLAGS_SOFT_ALPA_FALLBACK: u32 = 1 << 21;
/// `MPI_CFG_FC_PORT_0_FLAGS_PORT_OFFLINE`.
pub const MPI_CFG_FC_PORT_0_FLAGS_PORT_OFFLINE: u32 = 1 << 22;
/// `MPI_CFG_FC_PORT_0_FLAGS_TGT_MODE_OXID`.
pub const MPI_CFG_FC_PORT_0_FLAGS_TGT_MODE_OXID: u32 = 1 << 23;
/// `MPI_CFG_FC_PORT_0_FLAGS_VERBOSE_RESCAN`.
pub const MPI_CFG_FC_PORT_0_FLAGS_VERBOSE_RESCAN: u32 = 1 << 24;
/// `MPI_CFG_FC_PORT_0_FLAGS_FORCE_NOSEEPROM_WWNS`.
pub const MPI_CFG_FC_PORT_0_FLAGS_FORCE_NOSEEPROM_WWNS: u32 = 1 << 25;
/// `MPI_CFG_FC_PORT_0_FLAGS_IMMEDIATE_ERROR`.
pub const MPI_CFG_FC_PORT_0_FLAGS_IMMEDIATE_ERROR: u32 = 1 << 26;
/// `MPI_CFG_FC_PORT_0_FLAGS_EXT_FCP_STATUS_EN`.
pub const MPI_CFG_FC_PORT_0_FLAGS_EXT_FCP_STATUS_EN: u32 = 1 << 27;
/// `MPI_CFG_FC_PORT_0_FLAGS_REQ_PROT_LOG_BUS_ADDR`.
pub const MPI_CFG_FC_PORT_0_FLAGS_REQ_PROT_LOG_BUS_ADDR: u32 = 1 << 28;
/// `MPI_CFG_FC_PORT_0_FLAGS_REQ_PROT_LAN`.
pub const MPI_CFG_FC_PORT_0_FLAGS_REQ_PROT_LAN: u32 = 1 << 29;
/// `MPI_CFG_FC_PORT_0_FLAGS_REQ_PROT_TARGET`.
pub const MPI_CFG_FC_PORT_0_FLAGS_REQ_PROT_TARGET: u32 = 1 << 30;
/// `MPI_CFG_FC_PORT_0_FLAGS_REQ_PROT_INITIATOR`.
pub const MPI_CFG_FC_PORT_0_FLAGS_REQ_PROT_INITIATOR: u32 = 1 << 31;
/// `struct mpi_cfg_fc_port_pg1`.
#[derive(Clone, Copy, Default)]
#[repr(C, packed)]
pub struct MpiCfgFcPortPg1 {
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `flags`.
    pub flags: u32,
    /// `noseepromwwnn`.
    pub noseepromwwnn: u64,
    /// `noseepromwwpn`.
    pub noseepromwwpn: u64,
    /// `hard_alpa`.
    pub hard_alpa: u8,
    /// `link_config`.
    pub link_config: u8,
    /// `topology_config`.
    pub topology_config: u8,
    /// `alt_connector`.
    pub alt_connector: u8,
    /// `num_req_aliases`.
    pub num_req_aliases: u8,
    /// `rr_tov`.
    pub rr_tov: u8,
    /// `initiator_dev_to`.
    pub initiator_dev_to: u8,
    /// `initiator_lo_pend_to`.
    pub initiator_lo_pend_to: u8,
}
const _: () = {
    assert!(size_of::<MpiCfgFcPortPg1>() == 32);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg1, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg1, flags) == 4);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg1, noseepromwwnn) == 8);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg1, noseepromwwpn) == 16);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg1, hard_alpa) == 24);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg1, link_config) == 25);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg1, topology_config) == 26);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg1, alt_connector) == 27);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg1, num_req_aliases) == 28);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg1, rr_tov) == 29);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg1, initiator_dev_to) == 30);
    assert!(core::mem::offset_of!(MpiCfgFcPortPg1, initiator_lo_pend_to) == 31);
};
/// `MPI_CFG_FC_DEV_0_FLAGS_BUSADDR_VALID`.
pub const MPI_CFG_FC_DEV_0_FLAGS_BUSADDR_VALID: u8 = 1;
/// `MPI_CFG_FC_DEV_0_FLAGS_PLOGI_INVALID`.
pub const MPI_CFG_FC_DEV_0_FLAGS_PLOGI_INVALID: u8 = 1 << 1;
/// `MPI_CFG_FC_DEV_0_FLAGS_PRLI_INVALID`.
pub const MPI_CFG_FC_DEV_0_FLAGS_PRLI_INVALID: u8 = 1 << 2;
/// `struct mpi_cfg_fc_device_pg0`.
#[derive(Clone, Copy, Default)]
#[repr(C, packed)]
pub struct MpiCfgFcDevicePg0 {
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `wwnn`.
    pub wwnn: u64,
    /// `wwpn`.
    pub wwpn: u64,
    /// `port_id`.
    pub port_id: u32,
    /// `protocol`.
    pub protocol: u8,
    /// `flags`.
    pub flags: u8,
    /// `bb_credit`.
    pub bb_credit: u16,
    /// `max_rx_frame_size`.
    pub max_rx_frame_size: u16,
    /// `adisc_hard_alpa`.
    pub adisc_hard_alpa: u8,
    /// `port_nr`.
    pub port_nr: u8,
    /// `fc_ph_low_version`.
    pub fc_ph_low_version: u8,
    /// `fc_ph_high_version`.
    pub fc_ph_high_version: u8,
    /// `current_target_id`.
    pub current_target_id: u8,
    /// `current_bus`.
    pub current_bus: u8,
}
const _: () = {
    assert!(size_of::<MpiCfgFcDevicePg0>() == 36);
    assert!(core::mem::offset_of!(MpiCfgFcDevicePg0, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgFcDevicePg0, wwnn) == 4);
    assert!(core::mem::offset_of!(MpiCfgFcDevicePg0, wwpn) == 12);
    assert!(core::mem::offset_of!(MpiCfgFcDevicePg0, port_id) == 20);
    assert!(core::mem::offset_of!(MpiCfgFcDevicePg0, protocol) == 24);
    assert!(core::mem::offset_of!(MpiCfgFcDevicePg0, flags) == 25);
    assert!(core::mem::offset_of!(MpiCfgFcDevicePg0, bb_credit) == 26);
    assert!(core::mem::offset_of!(MpiCfgFcDevicePg0, max_rx_frame_size) == 28);
    assert!(core::mem::offset_of!(MpiCfgFcDevicePg0, adisc_hard_alpa) == 30);
    assert!(core::mem::offset_of!(MpiCfgFcDevicePg0, port_nr) == 31);
    assert!(core::mem::offset_of!(MpiCfgFcDevicePg0, fc_ph_low_version) == 32);
    assert!(core::mem::offset_of!(MpiCfgFcDevicePg0, fc_ph_high_version) == 33);
    assert!(core::mem::offset_of!(MpiCfgFcDevicePg0, current_target_id) == 34);
    assert!(core::mem::offset_of!(MpiCfgFcDevicePg0, current_bus) == 35);
};
/// `MPI_CFG_RAID_VOL_0_SETTINGS_WRITE_CACHE_EN`.
pub const MPI_CFG_RAID_VOL_0_SETTINGS_WRITE_CACHE_EN: u16 = 1;
/// `MPI_CFG_RAID_VOL_0_SETTINGS_OFFLINE_SMART_ERR`.
pub const MPI_CFG_RAID_VOL_0_SETTINGS_OFFLINE_SMART_ERR: u16 = 1 << 1;
/// `MPI_CFG_RAID_VOL_0_SETTINGS_OFFLINE_SMART`.
pub const MPI_CFG_RAID_VOL_0_SETTINGS_OFFLINE_SMART: u16 = 1 << 2;
/// `MPI_CFG_RAID_VOL_0_SETTINGS_AUTO_SWAP`.
pub const MPI_CFG_RAID_VOL_0_SETTINGS_AUTO_SWAP: u16 = 1 << 3;
/// `MPI_CFG_RAID_VOL_0_SETTINGS_HI_PRI_RESYNC`.
pub const MPI_CFG_RAID_VOL_0_SETTINGS_HI_PRI_RESYNC: u16 = 1 << 4;
/// `MPI_CFG_RAID_VOL_0_SETTINGS_PROD_SUFFIX`.
pub const MPI_CFG_RAID_VOL_0_SETTINGS_PROD_SUFFIX: u16 = 1 << 5;
/// `MPI_CFG_RAID_VOL_0_SETTINGS_FAST_SCRUB`: obsolete.
pub const MPI_CFG_RAID_VOL_0_SETTINGS_FAST_SCRUB: u16 = 1 << 6;
/// `MPI_CFG_RAID_VOL_0_SETTINGS_DEFAULTS`.
pub const MPI_CFG_RAID_VOL_0_SETTINGS_DEFAULTS: u16 = 1 << 15;
/// `struct mpi_raid_settings`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiRaidSettings {
    /// `volume_settings`.
    pub volume_settings: u16,
    /// `hot_spare_pool`.
    pub hot_spare_pool: u8,
    /// `reserved2`.
    pub reserved2: u8,
}
const _: () = {
    assert!(size_of::<MpiRaidSettings>() == 4);
    assert!(core::mem::offset_of!(MpiRaidSettings, volume_settings) == 0);
    assert!(core::mem::offset_of!(MpiRaidSettings, hot_spare_pool) == 2);
};
/// `MPI_CFG_RAID_VOL_0_STATUS_ENABLED`.
pub const MPI_CFG_RAID_VOL_0_STATUS_ENABLED: u8 = 1;
/// `MPI_CFG_RAID_VOL_0_STATUS_QUIESCED`.
pub const MPI_CFG_RAID_VOL_0_STATUS_QUIESCED: u8 = 1 << 1;
/// `MPI_CFG_RAID_VOL_0_STATUS_RESYNCING`.
pub const MPI_CFG_RAID_VOL_0_STATUS_RESYNCING: u8 = 1 << 2;
/// `MPI_CFG_RAID_VOL_0_STATUS_ACTIVE`.
pub const MPI_CFG_RAID_VOL_0_STATUS_ACTIVE: u8 = 1 << 3;
/// `MPI_CFG_RAID_VOL_0_STATUS_BADBLOCK_FULL`.
pub const MPI_CFG_RAID_VOL_0_STATUS_BADBLOCK_FULL: u8 = 1 << 4;
/// `MPI_CFG_RAID_VOL_0_STATE_OPTIMAL`.
pub const MPI_CFG_RAID_VOL_0_STATE_OPTIMAL: u8 = 0x00;
/// `MPI_CFG_RAID_VOL_0_STATE_DEGRADED`.
pub const MPI_CFG_RAID_VOL_0_STATE_DEGRADED: u8 = 0x01;
/// `MPI_CFG_RAID_VOL_0_STATE_FAILED`.
pub const MPI_CFG_RAID_VOL_0_STATE_FAILED: u8 = 0x02;
/// `MPI_CFG_RAID_VOL_0_STATE_MISSING`.
pub const MPI_CFG_RAID_VOL_0_STATE_MISSING: u8 = 0x03;
/// `MPI_CFG_RAID_VOL_0_INACTIVE_UNKNOWN`.
pub const MPI_CFG_RAID_VOL_0_INACTIVE_UNKNOWN: u8 = 0x00;
/// `MPI_CFG_RAID_VOL_0_INACTIVE_STALE_META`.
pub const MPI_CFG_RAID_VOL_0_INACTIVE_STALE_META: u8 = 0x01;
/// `MPI_CFG_RAID_VOL_0_INACTIVE_FOREIGN_VOL`.
pub const MPI_CFG_RAID_VOL_0_INACTIVE_FOREIGN_VOL: u8 = 0x02;
/// `MPI_CFG_RAID_VOL_0_INACTIVE_NO_RESOURCES`.
pub const MPI_CFG_RAID_VOL_0_INACTIVE_NO_RESOURCES: u8 = 0x03;
/// `MPI_CFG_RAID_VOL_0_INACTIVE_CLONED_VOL`.
pub const MPI_CFG_RAID_VOL_0_INACTIVE_CLONED_VOL: u8 = 0x04;
/// `MPI_CFG_RAID_VOL_0_INACTIVE_INSUF_META`.
pub const MPI_CFG_RAID_VOL_0_INACTIVE_INSUF_META: u8 = 0x05;
/// `struct mpi_cfg_raid_vol_pg0`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgRaidVolPg0 {
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `volume_id`.
    pub volume_id: u8,
    /// `volume_bus`.
    pub volume_bus: u8,
    /// `volume_ioc`.
    pub volume_ioc: u8,
    /// `volume_type`.
    pub volume_type: u8,
    /// `volume_status`.
    pub volume_status: u8,
    /// `volume_state`.
    pub volume_state: u8,
    /// `_reserved1`.
    pub _reserved1: u16,
    /// `settings`.
    pub settings: MpiRaidSettings,
    /// `max_lba`.
    pub max_lba: u32,
    /// `_reserved2`.
    pub _reserved2: u32,
    /// `stripe_size`.
    pub stripe_size: u32,
    /// `_reserved3`.
    pub _reserved3: u32,
    /// `_reserved4`.
    pub _reserved4: u32,
    /// `num_phys_disks`.
    pub num_phys_disks: u8,
    /// `data_scrub_rate`.
    pub data_scrub_rate: u8,
    /// `resync_rate`.
    pub resync_rate: u8,
    /// `inactive_status`.
    pub inactive_status: u8,
}
const _: () = {
    assert!(size_of::<MpiCfgRaidVolPg0>() == 40);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg0, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg0, volume_id) == 4);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg0, volume_bus) == 5);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg0, volume_ioc) == 6);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg0, volume_type) == 7);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg0, volume_status) == 8);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg0, volume_state) == 9);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg0, settings) == 12);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg0, max_lba) == 16);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg0, stripe_size) == 24);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg0, num_phys_disks) == 36);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg0, data_scrub_rate) == 37);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg0, resync_rate) == 38);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg0, inactive_status) == 39);
};
/// `struct mpi_cfg_raid_vol_pg0_physdisk`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgRaidVolPg0Physdisk {
    /// `reserved`.
    pub reserved: u16,
    /// `phys_disk_map`.
    pub phys_disk_map: u8,
    /// `phys_disk_num`.
    pub phys_disk_num: u8,
}
const _: () = {
    assert!(size_of::<MpiCfgRaidVolPg0Physdisk>() == 4);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg0Physdisk, phys_disk_map) == 2);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg0Physdisk, phys_disk_num) == 3);
};
/// `struct mpi_cfg_raid_vol_pg1`.
#[derive(Clone, Copy, Default)]
#[repr(C, packed)]
pub struct MpiCfgRaidVolPg1 {
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `volume_id`.
    pub volume_id: u8,
    /// `volume_bus`.
    pub volume_bus: u8,
    /// `volume_ioc`.
    pub volume_ioc: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `guid`.
    pub guid: [u8; 24],
    /// `name`.
    pub name: [u8; 32],
    /// `wwid`.
    pub wwid: u64,
    /// `reserved2`.
    pub reserved2: u32,
    /// `reserved3`.
    pub reserved3: u32,
}
const _: () = {
    assert!(size_of::<MpiCfgRaidVolPg1>() == 80);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg1, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg1, volume_id) == 4);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg1, volume_bus) == 5);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg1, volume_ioc) == 6);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg1, guid) == 8);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg1, name) == 32);
    assert!(core::mem::offset_of!(MpiCfgRaidVolPg1, wwid) == 64);
};
/// `MPI_CFG_RAID_PHYDISK_0_ENCTYPE_NONE`.
pub const MPI_CFG_RAID_PHYDISK_0_ENCTYPE_NONE: u8 = 0x0;
/// `MPI_CFG_RAID_PHYDISK_0_ENCTYPE_SAFTE`.
pub const MPI_CFG_RAID_PHYDISK_0_ENCTYPE_SAFTE: u8 = 0x1;
/// `MPI_CFG_RAID_PHYDISK_0_ENCTYPE_SES`.
pub const MPI_CFG_RAID_PHYDISK_0_ENCTYPE_SES: u8 = 0x2;
/// `MPI_CFG_RAID_PHYDISK_0_STATUS_OUTOFSYNC`.
pub const MPI_CFG_RAID_PHYDISK_0_STATUS_OUTOFSYNC: u8 = 1;
/// `MPI_CFG_RAID_PHYDISK_0_STATUS_QUIESCED`.
pub const MPI_CFG_RAID_PHYDISK_0_STATUS_QUIESCED: u8 = 1 << 1;
/// `MPI_CFG_RAID_PHYDISK_0_STATE_ONLINE`.
pub const MPI_CFG_RAID_PHYDISK_0_STATE_ONLINE: u8 = 0x00;
/// `MPI_CFG_RAID_PHYDISK_0_STATE_MISSING`.
pub const MPI_CFG_RAID_PHYDISK_0_STATE_MISSING: u8 = 0x01;
/// `MPI_CFG_RAID_PHYDISK_0_STATE_INCOMPAT`.
pub const MPI_CFG_RAID_PHYDISK_0_STATE_INCOMPAT: u8 = 0x02;
/// `MPI_CFG_RAID_PHYDISK_0_STATE_FAILED`.
pub const MPI_CFG_RAID_PHYDISK_0_STATE_FAILED: u8 = 0x03;
/// `MPI_CFG_RAID_PHYDISK_0_STATE_INIT`.
pub const MPI_CFG_RAID_PHYDISK_0_STATE_INIT: u8 = 0x04;
/// `MPI_CFG_RAID_PHYDISK_0_STATE_OFFLINE`.
pub const MPI_CFG_RAID_PHYDISK_0_STATE_OFFLINE: u8 = 0x05;
/// `MPI_CFG_RAID_PHYDISK_0_STATE_HOSTFAIL`.
pub const MPI_CFG_RAID_PHYDISK_0_STATE_HOSTFAIL: u8 = 0x06;
/// `MPI_CFG_RAID_PHYDISK_0_STATE_OTHER`.
pub const MPI_CFG_RAID_PHYDISK_0_STATE_OTHER: u8 = 0xff;
/// `struct mpi_cfg_raid_physdisk_pg0`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgRaidPhysdiskPg0 {
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `phys_disk_id`.
    pub phys_disk_id: u8,
    /// `phys_disk_bus`.
    pub phys_disk_bus: u8,
    /// `phys_disk_ioc`.
    pub phys_disk_ioc: u8,
    /// `phys_disk_num`.
    pub phys_disk_num: u8,
    /// `enc_id`.
    pub enc_id: u8,
    /// `enc_bus`.
    pub enc_bus: u8,
    /// `hot_spare_pool`.
    pub hot_spare_pool: u8,
    /// `enc_type`.
    pub enc_type: u8,
    /// `reserved1`.
    pub reserved1: u32,
    /// `ext_disk_id`.
    pub ext_disk_id: [u8; 8],
    /// `disk_id`.
    pub disk_id: [u8; 16],
    /// `vendor_id`.
    pub vendor_id: [u8; 8],
    /// `product_id`.
    pub product_id: [u8; 16],
    /// `product_rev`.
    pub product_rev: [u8; 4],
    /// `info`.
    pub info: [u8; 32],
    /// `phys_disk_status`.
    pub phys_disk_status: u8,
    /// `phys_disk_state`.
    pub phys_disk_state: u8,
    /// `reserved2`.
    pub reserved2: u16,
    /// `max_lba`.
    pub max_lba: u32,
    /// `error_cdb_byte`.
    pub error_cdb_byte: u8,
    /// `error_sense_key`.
    pub error_sense_key: u8,
    /// `reserved3`.
    pub reserved3: u16,
    /// `error_count`.
    pub error_count: u16,
    /// `error_asc`.
    pub error_asc: u8,
    /// `error_ascq`.
    pub error_ascq: u8,
    /// `smart_count`.
    pub smart_count: u16,
    /// `smart_asc`.
    pub smart_asc: u8,
    /// `smart_ascq`.
    pub smart_ascq: u8,
}
const _: () = {
    assert!(size_of::<MpiCfgRaidPhysdiskPg0>() == 120);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, phys_disk_id) == 4);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, phys_disk_bus) == 5);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, phys_disk_ioc) == 6);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, phys_disk_num) == 7);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, enc_id) == 8);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, enc_bus) == 9);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, hot_spare_pool) == 10);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, enc_type) == 11);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, ext_disk_id) == 16);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, disk_id) == 24);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, vendor_id) == 40);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, product_id) == 48);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, product_rev) == 64);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, info) == 68);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, phys_disk_status) == 100);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, phys_disk_state) == 101);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, max_lba) == 104);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, error_cdb_byte) == 108);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, error_sense_key) == 109);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, error_count) == 112);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, error_asc) == 114);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, error_ascq) == 115);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, smart_count) == 116);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, smart_asc) == 118);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg0, smart_ascq) == 119);
};
/// `struct mpi_cfg_raid_physdisk_pg1`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgRaidPhysdiskPg1 {
    /// `config_header`.
    pub config_header: MpiCfgHdr,
    /// `num_phys_disk_paths`.
    pub num_phys_disk_paths: u8,
    /// `phys_disk_num`.
    pub phys_disk_num: u8,
    /// `reserved1`.
    pub reserved1: u16,
    /// `reserved2`.
    pub reserved2: u32,
}
const _: () = {
    assert!(size_of::<MpiCfgRaidPhysdiskPg1>() == 12);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg1, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg1, num_phys_disk_paths) == 4);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPg1, phys_disk_num) == 5);
};
/// `MPI_CFG_RAID_PHYDISK_PATH_INVALID`.
pub const MPI_CFG_RAID_PHYDISK_PATH_INVALID: u16 = 1;
/// `MPI_CFG_RAID_PHYDISK_PATH_BROKEN`.
pub const MPI_CFG_RAID_PHYDISK_PATH_BROKEN: u16 = 1 << 1;
/// `struct mpi_cfg_raid_physdisk_path`.
#[derive(Clone, Copy, Default)]
#[repr(C, packed)]
pub struct MpiCfgRaidPhysdiskPath {
    /// `phys_disk_id`.
    pub phys_disk_id: u8,
    /// `phys_disk_bus`.
    pub phys_disk_bus: u8,
    /// `reserved1`.
    pub reserved1: u16,
    /// `wwwid`.
    pub wwwid: u64,
    /// `owner_wwid`.
    pub owner_wwid: u64,
    /// `ownder_id`.
    pub ownder_id: u8,
    /// `reserved2`.
    pub reserved2: u8,
    /// `flags`.
    pub flags: u16,
}
const _: () = {
    assert!(size_of::<MpiCfgRaidPhysdiskPath>() == 24);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPath, phys_disk_id) == 0);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPath, phys_disk_bus) == 1);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPath, wwwid) == 4);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPath, owner_wwid) == 12);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPath, ownder_id) == 20);
    assert!(core::mem::offset_of!(MpiCfgRaidPhysdiskPath, flags) == 22);
};
/// `struct mpi_cfg_sas_iou_pg0`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgSasIouPg0 {
    /// `config_header`.
    pub config_header: MpiEcfgHdr,
    /// `nvdata_version_default`.
    pub nvdata_version_default: u16,
    /// `nvdata_version_persistent`.
    pub nvdata_version_persistent: u16,
    /// `num_phys`.
    pub num_phys: u8,
    /// `_reserved1`.
    pub _reserved1: [u8; 3],
}
const _: () = {
    assert!(size_of::<MpiCfgSasIouPg0>() == 16);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg0, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg0, nvdata_version_default) == 8);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg0, nvdata_version_persistent) == 10);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg0, num_phys) == 12);
};
/// `struct mpi_cfg_sas_iou_pg0_phy`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgSasIouPg0Phy {
    /// `port`.
    pub port: u8,
    /// `port_flags`.
    pub port_flags: u8,
    /// `phy_flags`.
    pub phy_flags: u8,
    /// `negotiated_link_rate`.
    pub negotiated_link_rate: u8,
    /// `controller_phy_dev_info`.
    pub controller_phy_dev_info: u32,
    /// `attached_dev_handle`.
    pub attached_dev_handle: u16,
    /// `controller_dev_handle`.
    pub controller_dev_handle: u16,
    /// `discovery_status`.
    pub discovery_status: u32,
}
const _: () = {
    assert!(size_of::<MpiCfgSasIouPg0Phy>() == 16);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg0Phy, port) == 0);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg0Phy, port_flags) == 1);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg0Phy, phy_flags) == 2);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg0Phy, negotiated_link_rate) == 3);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg0Phy, controller_phy_dev_info) == 4);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg0Phy, attached_dev_handle) == 8);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg0Phy, controller_dev_handle) == 10);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg0Phy, discovery_status) == 12);
};
/// `struct mpi_cfg_sas_iou_pg1`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgSasIouPg1 {
    /// `config_header`.
    pub config_header: MpiEcfgHdr,
    /// `control_flags`.
    pub control_flags: u16,
    /// `max_sata_targets`.
    pub max_sata_targets: u16,
    /// `additional_control_flags`.
    pub additional_control_flags: u16,
    /// `_reserved1`.
    pub _reserved1: u16,
    /// `num_phys`.
    pub num_phys: u8,
    /// `max_sata_q_depth`.
    pub max_sata_q_depth: u8,
    /// `report_dev_missing_delay`.
    pub report_dev_missing_delay: u8,
    /// `io_dev_missing_delay`.
    pub io_dev_missing_delay: u8,
}
const _: () = {
    assert!(size_of::<MpiCfgSasIouPg1>() == 20);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg1, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg1, control_flags) == 8);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg1, max_sata_targets) == 10);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg1, additional_control_flags) == 12);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg1, num_phys) == 16);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg1, max_sata_q_depth) == 17);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg1, report_dev_missing_delay) == 18);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg1, io_dev_missing_delay) == 19);
};
/// `struct mpi_cfg_sas_iou_pg1_phy`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MpiCfgSasIouPg1Phy {
    /// `port`.
    pub port: u8,
    /// `port_flags`.
    pub port_flags: u8,
    /// `phy_flags`.
    pub phy_flags: u8,
    /// `max_min_link_rate`.
    pub max_min_link_rate: u8,
    /// `controller_phy_dev_info`.
    pub controller_phy_dev_info: u32,
    /// `max_target_port_connect_time`.
    pub max_target_port_connect_time: u16,
    /// `_reserved1`.
    pub _reserved1: u16,
}
const _: () = {
    assert!(size_of::<MpiCfgSasIouPg1Phy>() == 12);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg1Phy, port) == 0);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg1Phy, port_flags) == 1);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg1Phy, phy_flags) == 2);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg1Phy, max_min_link_rate) == 3);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg1Phy, controller_phy_dev_info) == 4);
    assert!(core::mem::offset_of!(MpiCfgSasIouPg1Phy, max_target_port_connect_time) == 8);
};
/// `MPI_CFG_SAS_DEV_ADDR_NEXT`.
pub const MPI_CFG_SAS_DEV_ADDR_NEXT: u32 = 0;
/// `MPI_CFG_SAS_DEV_ADDR_BUS`.
pub const MPI_CFG_SAS_DEV_ADDR_BUS: u32 = 1 << 28;
/// `MPI_CFG_SAS_DEV_ADDR_HANDLE`.
pub const MPI_CFG_SAS_DEV_ADDR_HANDLE: u32 = 2 << 28;
/// `MPI_CFG_SAS_DEV_0_DEVINFO_TYPE`.
pub const MPI_CFG_SAS_DEV_0_DEVINFO_TYPE: u32 = 0x7;
/// `MPI_CFG_SAS_DEV_0_DEVINFO_TYPE_NONE`.
pub const MPI_CFG_SAS_DEV_0_DEVINFO_TYPE_NONE: u32 = 0x0;
/// `MPI_CFG_SAS_DEV_0_DEVINFO_TYPE_END`.
pub const MPI_CFG_SAS_DEV_0_DEVINFO_TYPE_END: u32 = 0x1;
/// `MPI_CFG_SAS_DEV_0_DEVINFO_TYPE_EDGE_EXPANDER`.
pub const MPI_CFG_SAS_DEV_0_DEVINFO_TYPE_EDGE_EXPANDER: u32 = 0x2;
/// `MPI_CFG_SAS_DEV_0_DEVINFO_TYPE_FANOUT_EXPANDER`.
pub const MPI_CFG_SAS_DEV_0_DEVINFO_TYPE_FANOUT_EXPANDER: u32 = 0x3;
/// `MPI_CFG_SAS_DEV_0_DEVINFO_SATA_HOST`.
pub const MPI_CFG_SAS_DEV_0_DEVINFO_SATA_HOST: u32 = 1 << 3;
/// `MPI_CFG_SAS_DEV_0_DEVINFO_SMP_INITIATOR`.
pub const MPI_CFG_SAS_DEV_0_DEVINFO_SMP_INITIATOR: u32 = 1 << 4;
/// `MPI_CFG_SAS_DEV_0_DEVINFO_STP_INITIATOR`.
pub const MPI_CFG_SAS_DEV_0_DEVINFO_STP_INITIATOR: u32 = 1 << 5;
/// `MPI_CFG_SAS_DEV_0_DEVINFO_SSP_INITIATOR`.
pub const MPI_CFG_SAS_DEV_0_DEVINFO_SSP_INITIATOR: u32 = 1 << 6;
/// `MPI_CFG_SAS_DEV_0_DEVINFO_SATA_DEVICE`.
pub const MPI_CFG_SAS_DEV_0_DEVINFO_SATA_DEVICE: u32 = 1 << 7;
/// `MPI_CFG_SAS_DEV_0_DEVINFO_SMP_TARGET`.
pub const MPI_CFG_SAS_DEV_0_DEVINFO_SMP_TARGET: u32 = 1 << 8;
/// `MPI_CFG_SAS_DEV_0_DEVINFO_STP_TARGET`.
pub const MPI_CFG_SAS_DEV_0_DEVINFO_STP_TARGET: u32 = 1 << 9;
/// `MPI_CFG_SAS_DEV_0_DEVINFO_SSP_TARGET`.
pub const MPI_CFG_SAS_DEV_0_DEVINFO_SSP_TARGET: u32 = 1 << 10;
/// `MPI_CFG_SAS_DEV_0_DEVINFO_DIRECT_ATTACHED`.
pub const MPI_CFG_SAS_DEV_0_DEVINFO_DIRECT_ATTACHED: u32 = 1 << 11;
/// `MPI_CFG_SAS_DEV_0_DEVINFO_LSI_DEVICE`.
pub const MPI_CFG_SAS_DEV_0_DEVINFO_LSI_DEVICE: u32 = 1 << 12;
/// `MPI_CFG_SAS_DEV_0_DEVINFO_ATAPI_DEVICE`.
pub const MPI_CFG_SAS_DEV_0_DEVINFO_ATAPI_DEVICE: u32 = 1 << 13;
/// `MPI_CFG_SAS_DEV_0_DEVINFO_SEP_DEVICE`.
pub const MPI_CFG_SAS_DEV_0_DEVINFO_SEP_DEVICE: u32 = 1 << 14;
/// `MPI_CFG_SAS_DEV_0_FLAGS_DEV_PRESENT`.
pub const MPI_CFG_SAS_DEV_0_FLAGS_DEV_PRESENT: u16 = 1;
/// `MPI_CFG_SAS_DEV_0_FLAGS_DEV_MAPPED`.
pub const MPI_CFG_SAS_DEV_0_FLAGS_DEV_MAPPED: u16 = 1 << 1;
/// `MPI_CFG_SAS_DEV_0_FLAGS_DEV_MAPPED_PERSISTENT`.
pub const MPI_CFG_SAS_DEV_0_FLAGS_DEV_MAPPED_PERSISTENT: u16 = 1 << 2;
/// `MPI_CFG_SAS_DEV_0_FLAGS_SATA_PORT_SELECTOR`.
pub const MPI_CFG_SAS_DEV_0_FLAGS_SATA_PORT_SELECTOR: u16 = 1 << 3;
/// `MPI_CFG_SAS_DEV_0_FLAGS_SATA_FUA`.
pub const MPI_CFG_SAS_DEV_0_FLAGS_SATA_FUA: u16 = 1 << 4;
/// `MPI_CFG_SAS_DEV_0_FLAGS_SATA_NCQ`.
pub const MPI_CFG_SAS_DEV_0_FLAGS_SATA_NCQ: u16 = 1 << 5;
/// `MPI_CFG_SAS_DEV_0_FLAGS_SATA_SMART`.
pub const MPI_CFG_SAS_DEV_0_FLAGS_SATA_SMART: u16 = 1 << 6;
/// `MPI_CFG_SAS_DEV_0_FLAGS_SATA_LBA48`.
pub const MPI_CFG_SAS_DEV_0_FLAGS_SATA_LBA48: u16 = 1 << 7;
/// `MPI_CFG_SAS_DEV_0_FLAGS_UNSUPPORTED`.
pub const MPI_CFG_SAS_DEV_0_FLAGS_UNSUPPORTED: u16 = 1 << 8;
/// `MPI_CFG_SAS_DEV_0_FLAGS_SATA_SETTINGS`.
pub const MPI_CFG_SAS_DEV_0_FLAGS_SATA_SETTINGS: u16 = 1 << 9;
/// `struct mpi_cfg_sas_dev_pg0`.
#[derive(Clone, Copy, Default)]
#[repr(C, packed)]
pub struct MpiCfgSasDevPg0 {
    /// `config_header`.
    pub config_header: MpiEcfgHdr,
    /// `slot`.
    pub slot: u16,
    /// `enc_handle`.
    pub enc_handle: u16,
    /// `sas_addr`.
    pub sas_addr: u64,
    /// `parent_dev_handle`.
    pub parent_dev_handle: u16,
    /// `phy_num`.
    pub phy_num: u8,
    /// `access_status`.
    pub access_status: u8,
    /// `dev_handle`.
    pub dev_handle: u16,
    /// `target`.
    pub target: u8,
    /// `bus`.
    pub bus: u8,
    /// `device_info`.
    pub device_info: u32,
    /// `flags`.
    pub flags: u16,
    /// `physical_port`.
    pub physical_port: u8,
    /// `reserved`.
    pub reserved: u8,
}
const _: () = {
    assert!(size_of::<MpiCfgSasDevPg0>() == 36);
    assert!(core::mem::offset_of!(MpiCfgSasDevPg0, config_header) == 0);
    assert!(core::mem::offset_of!(MpiCfgSasDevPg0, slot) == 8);
    assert!(core::mem::offset_of!(MpiCfgSasDevPg0, enc_handle) == 10);
    assert!(core::mem::offset_of!(MpiCfgSasDevPg0, sas_addr) == 12);
    assert!(core::mem::offset_of!(MpiCfgSasDevPg0, parent_dev_handle) == 20);
    assert!(core::mem::offset_of!(MpiCfgSasDevPg0, phy_num) == 22);
    assert!(core::mem::offset_of!(MpiCfgSasDevPg0, access_status) == 23);
    assert!(core::mem::offset_of!(MpiCfgSasDevPg0, dev_handle) == 24);
    assert!(core::mem::offset_of!(MpiCfgSasDevPg0, target) == 26);
    assert!(core::mem::offset_of!(MpiCfgSasDevPg0, bus) == 27);
    assert!(core::mem::offset_of!(MpiCfgSasDevPg0, device_info) == 28);
    assert!(core::mem::offset_of!(MpiCfgSasDevPg0, flags) == 32);
    assert!(core::mem::offset_of!(MpiCfgSasDevPg0, physical_port) == 34);
};

/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doorbell_function_and_dwords() {
        // MPI_FUNCTION_HANDSHAKE with a 3-dword message, as mpi_handshake_send writes it.
        let db = mpi_doorbell_function(MPI_FUNCTION_IOC_FACTS as u32) | mpi_doorbell_dwords(5);
        assert_eq!(db, 0x0305_0000);
        assert_eq!(mpi_doorbell_function(0x1ff), 0xff00_0000);
        assert_eq!(mpi_doorbell_dwords(0x1ff), 0x00ff_0000);
    }

    #[test]
    fn doorbell_states() {
        let ready: u32 = 0x1000_0000;
        assert_eq!(ready & MPI_DOORBELL_STATE, MPI_DOORBELL_STATE_READY);
        let fault: u32 = 0x4000_8111;
        assert_eq!(fault & MPI_DOORBELL_STATE, MPI_DOORBELL_STATE_FAULT);
        assert_eq!(fault & MPI_DOORBELL_FAULT, MPI_DOORBELL_FAULT_REQ_PCIPAR);
    }

    #[test]
    fn sas_phy_link_rates() {
        assert_eq!(
            mpi_evt_sasphy_link_cur(0x98),
            MPI_EVT_SASPHY_LINK_3_0GBPS as u8
        );
        assert_eq!(
            mpi_evt_sasphy_link_prev(0x98),
            MPI_EVT_SASPHY_LINK_1_5GBPS as u8
        );
        assert_eq!(mpi_cfg_spi_dev_2_data_pipe_select(2), 0x3 << 4);
    }

    #[test]
    fn frames_are_zero_by_default() {
        let m = MpiMsgScsiIo::default();
        assert_eq!({ m.cdb }, [0u8; MPI_CDB_LEN]);
        assert_eq!(size_of::<MpiMsgScsiIo>(), 48);
        assert_eq!(size_of::<MpiSge>(), 12);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ic/mpireg.h");
        let names = crate::reftest::assert_defines!(defs;
            MPI_DOORBELL,
            MPI_DOORBELL_STATE,
            MPI_DOORBELL_STATE_RESET,
            MPI_DOORBELL_STATE_READY,
            MPI_DOORBELL_STATE_OPER,
            MPI_DOORBELL_STATE_FAULT,
            MPI_DOORBELL_INUSE,
            MPI_DOORBELL_WHOINIT,
            MPI_DOORBELL_WHOINIT_NOONE,
            MPI_DOORBELL_WHOINIT_SYSBIOS,
            MPI_DOORBELL_WHOINIT_ROMBIOS,
            MPI_DOORBELL_WHOINIT_PCIPEER,
            MPI_DOORBELL_WHOINIT_DRIVER,
            MPI_DOORBELL_WHOINIT_MANUFACT,
            MPI_DOORBELL_FAULT,
            MPI_DOORBELL_FAULT_REQ_PCIPAR,
            MPI_DOORBELL_FAULT_REQ_PCIBUS,
            MPI_DOORBELL_FAULT_REP_PCIPAR,
            MPI_DOORBELL_FAULT_REP_PCIBUS,
            MPI_DOORBELL_FAULT_SND_PCIPAR,
            MPI_DOORBELL_FAULT_SND_PCIBUS,
            MPI_DOORBELL_FAULT_RCV_PCIPAR,
            MPI_DOORBELL_FAULT_RCV_PCIBUS,
            MPI_DOORBELL_FUNCTION_SHIFT,
            MPI_DOORBELL_FUNCTION_MASK,
            MPI_DOORBELL_DWORDS_SHIFT,
            MPI_DOORBELL_DWORDS_MASK,
            MPI_DOORBELL_DATA_MASK,
            MPI_WRITESEQ,
            MPI_WRITESEQ_VALUE,
            MPI_WRITESEQ_1,
            MPI_WRITESEQ_2,
            MPI_WRITESEQ_3,
            MPI_WRITESEQ_4,
            MPI_WRITESEQ_5,
            MPI_HOSTDIAG,
            MPI_HOSTDIAG_CLEARFBS,
            MPI_HOSTDIAG_POICB,
            MPI_HOSTDIAG_DWRE,
            MPI_HOSTDIAG_FBS,
            MPI_HOSTDIAG_RESET_HIST,
            MPI_HOSTDIAG_DIAGWR_EN,
            MPI_HOSTDIAG_RESET_ADAPTER,
            MPI_HOSTDIAG_DISABLE_ARM,
            MPI_HOSTDIAG_DIAGMEM_EN,
            MPI_TESTBASE,
            MPI_DIAGRWDATA,
            MPI_DIAGRWADDR,
            MPI_INTR_STATUS,
            MPI_INTR_STATUS_IOCDOORBELL,
            MPI_INTR_STATUS_REPLY,
            MPI_INTR_STATUS_DOORBELL,
            MPI_INTR_MASK,
            MPI_INTR_MASK_REPLY,
            MPI_INTR_MASK_DOORBELL,
            MPI_REQ_QUEUE,
            MPI_REPLY_QUEUE,
            MPI_REPLY_QUEUE_ADDRESS,
            MPI_REPLY_QUEUE_ADDRESS_MASK,
            MPI_REPLY_QUEUE_TYPE_MASK,
            MPI_REPLY_QUEUE_TYPE_INIT,
            MPI_REPLY_QUEUE_TYPE_TARGET,
            MPI_REPLY_QUEUE_TYPE_LAN,
            MPI_REPLY_QUEUE_CONTEXT,
            MPI_PRIREQ_QUEUE,
            MPI_SGE_FL_LAST,
            MPI_SGE_FL_EOB,
            MPI_SGE_FL_TYPE,
            MPI_SGE_FL_TYPE_SIMPLE,
            MPI_SGE_FL_TYPE_CHAIN,
            MPI_SGE_FL_TYPE_XACTCTX,
            MPI_SGE_FL_LOCAL,
            MPI_SGE_FL_DIR,
            MPI_SGE_FL_DIR_OUT,
            MPI_SGE_FL_DIR_IN,
            MPI_SGE_FL_SIZE,
            MPI_SGE_FL_SIZE_32,
            MPI_SGE_FL_SIZE_64,
            MPI_SGE_FL_EOL,
            MPI_SGE_FLAGS_IOC_TO_HOST,
            MPI_SGE_FLAGS_HOST_TO_IOC,
            MPI_FUNCTION_SCSI_IO_REQUEST,
            MPI_FUNCTION_SCSI_TASK_MGMT,
            MPI_FUNCTION_IOC_INIT,
            MPI_FUNCTION_IOC_FACTS,
            MPI_FUNCTION_CONFIG,
            MPI_FUNCTION_PORT_FACTS,
            MPI_FUNCTION_PORT_ENABLE,
            MPI_FUNCTION_EVENT_NOTIFICATION,
            MPI_FUNCTION_EVENT_ACK,
            MPI_FUNCTION_FW_DOWNLOAD,
            MPI_FUNCTION_TARGET_CMD_BUFFER_POST,
            MPI_FUNCTION_TARGET_ASSIST,
            MPI_FUNCTION_TARGET_STATUS_SEND,
            MPI_FUNCTION_TARGET_MODE_ABORT,
            MPI_FUNCTION_TARGET_FC_BUF_POST_LINK_SRVC,
            MPI_FUNCTION_TARGET_FC_RSP_LINK_SRVC,
            MPI_FUNCTION_TARGET_FC_EX_SEND_LINK_SRVC,
            MPI_FUNCTION_TARGET_FC_ABORT,
            MPI_FUNCTION_FC_LINK_SRVC_BUF_POST,
            MPI_FUNCTION_FC_LINK_SRVC_RSP,
            MPI_FUNCTION_FC_EX_LINK_SRVC_SEND,
            MPI_FUNCTION_FC_ABORT,
            MPI_FUNCTION_FW_UPLOAD,
            MPI_FUNCTION_FC_COMMON_TRANSPORT_SEND,
            MPI_FUNCTION_FC_PRIMITIVE_SEND,
            MPI_FUNCTION_RAID_ACTION,
            MPI_FUNCTION_RAID_SCSI_IO_PASSTHROUGH,
            MPI_FUNCTION_TOOLBOX,
            MPI_FUNCTION_SCSI_ENCLOSURE_PROCESSOR,
            MPI_FUNCTION_MAILBOX,
            MPI_FUNCTION_LAN_SEND,
            MPI_FUNCTION_LAN_RECEIVE,
            MPI_FUNCTION_LAN_RESET,
            MPI_FUNCTION_IOC_MESSAGE_UNIT_RESET,
            MPI_FUNCTION_IO_UNIT_RESET,
            MPI_FUNCTION_HANDSHAKE,
            MPI_FUNCTION_REPLY_FRAME_REMOVAL,
            MPI_REP_FLAGS_CONT,
            MPI_REP_IOCSTATUS_AVAIL,
            MPI_REP_IOCSTATUS,
            MPI_IOCSTATUS_SUCCESS,
            MPI_IOCSTATUS_INVALID_FUNCTION,
            MPI_IOCSTATUS_BUSY,
            MPI_IOCSTATUS_INVALID_SGL,
            MPI_IOCSTATUS_INTERNAL_ERROR,
            MPI_IOCSTATUS_RESERVED,
            MPI_IOCSTATUS_INSUFFICIENT_RESOURCES,
            MPI_IOCSTATUS_INVALID_FIELD,
            MPI_IOCSTATUS_INVALID_STATE,
            MPI_IOCSTATUS_OP_STATE_NOT_SUPPORTED,
            MPI_IOCSTATUS_CONFIG_INVALID_ACTION,
            MPI_IOCSTATUS_CONFIG_INVALID_TYPE,
            MPI_IOCSTATUS_CONFIG_INVALID_PAGE,
            MPI_IOCSTATUS_CONFIG_INVALID_DATA,
            MPI_IOCSTATUS_CONFIG_NO_DEFAULTS,
            MPI_IOCSTATUS_CONFIG_CANT_COMMIT,
            MPI_IOCSTATUS_SCSI_RECOVERED_ERROR,
            MPI_IOCSTATUS_SCSI_INVALID_BUS,
            MPI_IOCSTATUS_SCSI_INVALID_TARGETID,
            MPI_IOCSTATUS_SCSI_DEVICE_NOT_THERE,
            MPI_IOCSTATUS_SCSI_DATA_OVERRUN,
            MPI_IOCSTATUS_SCSI_DATA_UNDERRUN,
            MPI_IOCSTATUS_SCSI_IO_DATA_ERROR,
            MPI_IOCSTATUS_SCSI_PROTOCOL_ERROR,
            MPI_IOCSTATUS_SCSI_TASK_TERMINATED,
            MPI_IOCSTATUS_SCSI_RESIDUAL_MISMATCH,
            MPI_IOCSTATUS_SCSI_TASK_MGMT_FAILED,
            MPI_IOCSTATUS_SCSI_IOC_TERMINATED,
            MPI_IOCSTATUS_SCSI_EXT_TERMINATED,
            MPI_IOCSTATUS_EEDP_GUARD_ERROR,
            MPI_IOCSTATUS_EEDP_REF_TAG_ERROR,
            MPI_IOCSTATUS_EEDP_APP_TAG_ERROR,
            MPI_IOCSTATUS_TARGET_PRIORITY_IO,
            MPI_IOCSTATUS_TARGET_INVALID_PORT,
            MPI_IOCSTATUS_TARGET_INVALID_IOCINDEX,
            MPI_IOCSTATUS_TARGET_INVALID_IO_INDEX,
            MPI_IOCSTATUS_TARGET_ABORTED,
            MPI_IOCSTATUS_TARGET_NO_CONN_RETRYABLE,
            MPI_IOCSTATUS_TARGET_NO_CONNECTION,
            MPI_IOCSTATUS_TARGET_XFER_COUNT_MISMATCH,
            MPI_IOCSTATUS_TARGET_STS_DATA_NOT_SENT,
            MPI_IOCSTATUS_TARGET_DATA_OFFSET_ERROR,
            MPI_IOCSTATUS_TARGET_TOO_MUCH_WRITE_DATA,
            MPI_IOCSTATUS_TARGET_IU_TOO_SHORT,
            MPI_IOCSTATUS_TARGET_FC_ABORTED,
            MPI_IOCSTATUS_TARGET_FC_RX_ID_INVALID,
            MPI_IOCSTATUS_TARGET_FC_DID_INVALID,
            MPI_IOCSTATUS_TARGET_FC_NODE_LOGGED_OUT,
            MPI_IOCSTATUS_FC_ABORTED,
            MPI_IOCSTATUS_FC_RX_ID_INVALID,
            MPI_IOCSTATUS_FC_DID_INVALID,
            MPI_IOCSTATUS_FC_NODE_LOGGED_OUT,
            MPI_IOCSTATUS_FC_EXCHANGE_CANCELED,
            MPI_IOCSTATUS_LAN_DEVICE_NOT_FOUND,
            MPI_IOCSTATUS_LAN_DEVICE_FAILURE,
            MPI_IOCSTATUS_LAN_TRANSMIT_ERROR,
            MPI_IOCSTATUS_LAN_TRANSMIT_ABORTED,
            MPI_IOCSTATUS_LAN_RECEIVE_ERROR,
            MPI_IOCSTATUS_LAN_RECEIVE_ABORTED,
            MPI_IOCSTATUS_LAN_PARTIAL_PACKET,
            MPI_IOCSTATUS_LAN_CANCELED,
            MPI_IOCSTATUS_SAS_SMP_REQUEST_FAILED,
            MPI_IOCSTATUS_SAS_SMP_DATA_OVERRUN,
            MPI_IOCSTATUS_INBAND_ABORTED,
            MPI_IOCSTATUS_INBAND_NO_CONNECTION,
            MPI_IOCSTATUS_DIAGNOSTIC_RELEASED,
            MPI_REP_IOCLOGINFO_TYPE,
            MPI_REP_IOCLOGINFO_TYPE_NONE,
            MPI_REP_IOCLOGINFO_TYPE_SCSI,
            MPI_REP_IOCLOGINFO_TYPE_FC,
            MPI_REP_IOCLOGINFO_TYPE_SAS,
            MPI_REP_IOCLOGINFO_TYPE_ISCSI,
            MPI_REP_IOCLOGINFO_DATA,
            MPI_EVENT_NONE,
            MPI_EVENT_LOG_DATA,
            MPI_EVENT_STATE_CHANGE,
            MPI_EVENT_UNIT_ATTENTION,
            MPI_EVENT_IOC_BUS_RESET,
            MPI_EVENT_EXT_BUS_RESET,
            MPI_EVENT_RESCAN,
            MPI_EVENT_LINK_STATUS_CHANGE,
            MPI_EVENT_LOOP_STATE_CHANGE,
            MPI_EVENT_LOGOUT,
            MPI_EVENT_EVENT_CHANGE,
            MPI_EVENT_INTEGRATED_RAID,
            MPI_EVENT_SCSI_DEVICE_STATUS_CHANGE,
            MPI_EVENT_ON_BUS_TIMER_EXPIRED,
            MPI_EVENT_QUEUE_FULL,
            MPI_EVENT_SAS_DEVICE_STATUS_CHANGE,
            MPI_EVENT_SAS_SES,
            MPI_EVENT_PERSISTENT_TABLE_FULL,
            MPI_EVENT_SAS_PHY_LINK_STATUS,
            MPI_EVENT_SAS_DISCOVERY_ERROR,
            MPI_EVENT_IR_RESYNC_UPDATE,
            MPI_EVENT_IR2,
            MPI_EVENT_SAS_DISCOVERY,
            MPI_EVENT_LOG_ENTRY_ADDED,
            MPI_WHOINIT_NOONE,
            MPI_WHOINIT_SYSTEM_BIOS,
            MPI_WHOINIT_ROM_BIOS,
            MPI_WHOINIT_PCI_PEER,
            MPI_WHOINIT_HOST_DRIVER,
            MPI_WHOINIT_MANUFACTURER,
            MPI_PAGE_ADDRESS_FC_BTID,
            MPI_IOCINIT_F_DISCARD_FW,
            MPI_IOCINIT_F_ENABLE_HOST_FIFO,
            MPI_IOCINIT_F_HOST_PG_BUF_PERSIST,
            MPI_IOCFACTS_EXCEPT_CONFIG_CHECKSUM_FAIL,
            MPI_IOCFACTS_EXCEPT_RAID_CONFIG_INVALID,
            MPI_IOCFACTS_EXCEPT_FW_CHECKSUM_FAIL,
            MPI_IOCFACTS_EXCEPT_PERSISTENT_TABLE_FULL,
            MPI_IOCFACTS_FLAGS_FW_DOWNLOAD_BOOT,
            MPI_IOCFACTS_FLAGS_REPLY_FIFO_HOST_SIGNAL,
            MPI_IOCFACTS_FLAGS_HOST_PAGE_BUFFER_PERSISTENT,
            MPI_IOCFACTS_CAPABILITY_HIGH_PRI_Q,
            MPI_IOCFACTS_CAPABILITY_REPLY_HOST_SIGNAL,
            MPI_IOCFACTS_CAPABILITY_QUEUE_FULL_HANDLING,
            MPI_IOCFACTS_CAPABILITY_DIAG_TRACE_BUFFER,
            MPI_IOCFACTS_CAPABILITY_SNAPSHOT_BUFFER,
            MPI_IOCFACTS_CAPABILITY_EXTENDED_BUFFER,
            MPI_IOCFACTS_CAPABILITY_EEDP,
            MPI_IOCFACTS_CAPABILITY_BIDIRECTIONAL,
            MPI_IOCFACTS_CAPABILITY_MULTICAST,
            MPI_IOCFACTS_CAPABILITY_SCSIIO32,
            MPI_IOCFACTS_CAPABILITY_NO_SCSIIO16,
            MPI_PORTFACTS_PORTTYPE_INACTIVE,
            MPI_PORTFACTS_PORTTYPE_SCSI,
            MPI_PORTFACTS_PORTTYPE_FC,
            MPI_PORTFACTS_PORTTYPE_ISCSI,
            MPI_PORTFACTS_PORTTYPE_SAS,
            MPI_PORTFACTS_PROTOCOL_LOGBUSADDR,
            MPI_PORTFACTS_PROTOCOL_LAN,
            MPI_PORTFACTS_PROTOCOL_TARGET,
            MPI_PORTFACTS_PROTOCOL_INITIATOR,
            MPI_EVENT_SWITCH_ON,
            MPI_EVENT_SWITCH_OFF,
            MPI_EVENT_ACK_REQUIRED,
            MPI_EVENT_FLAGS_REPLY_KEPT,
            MPI_EVT_LINK_STATUS_CHANGE_OFFLINE,
            MPI_EVT_LINK_STATUS_CHANGE_ACTIVE,
            MPI_EVT_LOOP_STATUS_CHANGE_TYPE_LIP,
            MPI_EVT_LOOP_STATUS_CHANGE_TYPE_LPE,
            MPI_EVT_LOOP_STATUS_CHANGE_TYPE_LPB,
            MPI_EVT_SASPHY_LINK_ENABLED,
            MPI_EVT_SASPHY_LINK_DISABLED,
            MPI_EVT_SASPHY_LINK_NEGFAIL,
            MPI_EVT_SASPHY_LINK_SATAOOB,
            MPI_EVT_SASPHY_LINK_1_5GBPS,
            MPI_EVT_SASPHY_LINK_3_0GBPS,
            MPI_EVT_SASCH_REASON_ADDED,
            MPI_EVT_SASCH_REASON_NOT_RESPONDING,
            MPI_EVT_SASCH_REASON_SMART_DATA,
            MPI_EVT_SASCH_REASON_NO_PERSIST_ADDED,
            MPI_EVT_SASCH_REASON_UNSUPPORTED,
            MPI_EVT_SASCH_REASON_INTERNAL_RESET,
            MPI_EVT_SASCH_INFO_ATAPI,
            MPI_EVT_SASCH_INFO_LSI,
            MPI_EVT_SASCH_INFO_DIRECT_ATTACHED,
            MPI_EVT_SASCH_INFO_SSP,
            MPI_EVT_SASCH_INFO_STP,
            MPI_EVT_SASCH_INFO_SMP,
            MPI_EVT_SASCH_INFO_SATA,
            MPI_EVT_SASCH_INFO_SSP_INITIATOR,
            MPI_EVT_SASCH_INFO_STP_INITIATOR,
            MPI_EVT_SASCH_INFO_SMP_INITIATOR,
            MPI_EVT_SASCH_INFO_SATA_HOST,
            MPI_EVT_SASCH_INFO_TYPE_MASK,
            MPI_EVT_SASCH_INFO_TYPE_NONE,
            MPI_EVT_SASCH_INFO_TYPE_END,
            MPI_EVT_SASCH_INFO_TYPE_EDGE,
            MPI_EVT_SASCH_INFO_TYPE_FANOUT,
            MPI_FWUPLOAD_IMAGETYPE_IOC_FW,
            MPI_FWUPLOAD_IMAGETYPE_NV_FW,
            MPI_FWUPLOAD_IMAGETYPE_MPI_NV_FW,
            MPI_FWUPLOAD_IMAGETYPE_NV_DATA,
            MPI_FWUPLOAD_IMAGETYPE_BOOT,
            MPI_FWUPLOAD_IMAGETYPE_NV_BACKUP,
            MPI_SCSIIO_EEDP,
            MPI_SCSIIO_CMD_DATA_DIR,
            MPI_SCSIIO_SENSE_BUF_LOC,
            MPI_SCSIIO_SENSE_BUF_ADDR_WIDTH,
            MPI_SCSIIO_SENSE_BUF_ADDR_WIDTH_32,
            MPI_SCSIIO_SENSE_BUF_ADDR_WIDTH_64,
            MPI_SCSIIO_ATTR_SIMPLE_Q,
            MPI_SCSIIO_ATTR_HEAD_OF_Q,
            MPI_SCSIIO_ATTR_ORDERED_Q,
            MPI_SCSIIO_ATTR_ACA_Q,
            MPI_SCSIIO_ATTR_UNTAGGED,
            MPI_SCSIIO_ATTR_NO_DISCONNECT,
            MPI_SCSIIO_DIR_NONE,
            MPI_SCSIIO_DIR_WRITE,
            MPI_SCSIIO_DIR_READ,
            MPI_CDB_LEN,
            MPI_SCSIIO_ERR_STATE_AUTOSENSE_VALID,
            MPI_SCSIIO_ERR_STATE_AUTOSENSE_FAILED,
            MPI_SCSIIO_ERR_STATE_NO_SCSI_STATUS,
            MPI_SCSIIO_ERR_STATE_TERMINATED,
            MPI_SCSIIO_ERR_STATE_RESPONSE_INFO_VALID,
            MPI_SCSIIO_ERR_STATE_QUEUE_TAG_REJECTED,
            MPI_MSG_SCSI_TASK_TYPE_ABORT_TASK,
            MPI_MSG_SCSI_TASK_TYPE_ABRT_TASK_SET,
            MPI_MSG_SCSI_TASK_TYPE_TARGET_RESET,
            MPI_MSG_SCSI_TASK_TYPE_RESET_BUS,
            MPI_MSG_SCSI_TASK_TYPE_LOGICAL_UNIT_RESET,
            MPI_MSG_RAID_ACTION_STATUS,
            MPI_MSG_RAID_ACTION_INDICATOR_STRUCT,
            MPI_MSG_RAID_ACTION_CREATE_VOLUME,
            MPI_MSG_RAID_ACTION_DELETE_VOLUME,
            MPI_MSG_RAID_ACTION_DISABLE_VOLUME,
            MPI_MSG_RAID_ACTION_ENABLE_VOLUME,
            MPI_MSG_RAID_ACTION_QUIESCE_PHYSIO,
            MPI_MSG_RAID_ACTION_ENABLE_PHYSIO,
            MPI_MSG_RAID_ACTION_CH_VOL_SETTINGS,
            MPI_MSG_RAID_ACTION_PHYSDISK_OFFLINE,
            MPI_MSG_RAID_ACTION_PHYSDISK_ONLINE,
            MPI_MSG_RAID_ACTION_CH_PHYSDISK_SETTINGS,
            MPI_MSG_RAID_ACTION_CREATE_PHYSDISK,
            MPI_MSG_RAID_ACTION_DELETE_PHYSDISK,
            MPI_MSG_RAID_ACTION_PHYSDISK_FAIL,
            MPI_MSG_RAID_ACTION_ACTIVATE_VOLUME,
            MPI_MSG_RAID_ACTION_DEACTIVATE_VOLUME,
            MPI_MSG_RAID_ACTION_SET_RESYNC_RATE,
            MPI_MSG_RAID_ACTION_SET_SCRUB_RATE,
            MPI_MSG_RAID_ACTION_DEVICE_FW_UPDATE_MODE,
            MPI_MSG_RAID_ACTION_SET_VOL_NAME,
            MPI_RAID_ACTION_STATUS_OK,
            MPI_RAID_ACTION_STATUS_INVALID,
            MPI_RAID_ACTION_STATUS_FAILURE,
            MPI_RAID_ACTION_STATUS_IN_PROGRESS,
            MPI_CONFIG_REQ_PAGE_TYPE_ATTRIBUTE,
            MPI_CONFIG_REQ_PAGE_TYPE_MASK,
            MPI_CONFIG_REQ_PAGE_TYPE_IO_UNIT,
            MPI_CONFIG_REQ_PAGE_TYPE_IOC,
            MPI_CONFIG_REQ_PAGE_TYPE_BIOS,
            MPI_CONFIG_REQ_PAGE_TYPE_SCSI_SPI_PORT,
            MPI_CONFIG_REQ_PAGE_TYPE_SCSI_SPI_DEV,
            MPI_CONFIG_REQ_PAGE_TYPE_FC_PORT,
            MPI_CONFIG_REQ_PAGE_TYPE_FC_DEV,
            MPI_CONFIG_REQ_PAGE_TYPE_LAN,
            MPI_CONFIG_REQ_PAGE_TYPE_RAID_VOL,
            MPI_CONFIG_REQ_PAGE_TYPE_MANUFACTURING,
            MPI_CONFIG_REQ_PAGE_TYPE_RAID_PD,
            MPI_CONFIG_REQ_PAGE_TYPE_INBAND,
            MPI_CONFIG_REQ_PAGE_TYPE_EXTENDED,
            MPI_CONFIG_REQ_ACTION_PAGE_HEADER,
            MPI_CONFIG_REQ_ACTION_PAGE_READ_CURRENT,
            MPI_CONFIG_REQ_ACTION_PAGE_WRITE_CURRENT,
            MPI_CONFIG_REQ_ACTION_PAGE_DEFAULT,
            MPI_CONFIG_REQ_ACTION_PAGE_WRITE_NVRAM,
            MPI_CONFIG_REQ_ACTION_PAGE_READ_DEFAULT,
            MPI_CONFIG_REQ_ACTION_PAGE_READ_NVRAM,
            MPI_CONFIG_REQ_EXTPAGE_TYPE_SAS_IO_UNIT,
            MPI_CONFIG_REQ_EXTPAGE_TYPE_SAS_EXPANDER,
            MPI_CONFIG_REQ_EXTPAGE_TYPE_SAS_DEVICE,
            MPI_CONFIG_REQ_EXTPAGE_TYPE_SAS_PHY,
            MPI_CONFIG_REQ_EXTPAGE_TYPE_LOG,
            MPI_CFG_SPI_PORT_0_CAPABILITIES_PACKETIZED,
            MPI_CFG_SPI_PORT_0_CAPABILITIES_DT,
            MPI_CFG_SPI_PORT_0_CAPABILITIES_QAS,
            MPI_CFG_SPI_PORT_0_CAPABILITIES_IDP,
            MPI_CFG_SPI_PORT_0_CAPABILITIES_WIDTH,
            MPI_CFG_SPI_PORT_0_CAPABILITIES_WIDTH_NARROW,
            MPI_CFG_SPI_PORT_0_CAPABILITIES_WIDTH_WIDE,
            MPI_CFG_SPI_PORT_0_CAPABILITIES_AIP,
            MPI_CFG_SPI_PORT_0_SIGNAL_HVD,
            MPI_CFG_SPI_PORT_0_SIGNAL_SE,
            MPI_CFG_SPI_PORT_0_SIGNAL_LVD,
            MPI_CFG_SPI_PORT_0_CONNECTEDID_BUSFREE,
            MPI_CFG_SPI_PORT_0_CONNECTEDID_UNKNOWN,
            MPI_CFG_SPI_PORT_1_TARGCFG_TARGET_ONLY,
            MPI_CFG_SPI_PORT_1_TARGCFG_INIT_TARGET,
            MPI_CFG_SPI_PORT_2_DEV_FLAG_DISCONNECT_EN,
            MPI_CFG_SPI_PORT_2_DEV_FLAG_SCAN_ID_EN,
            MPI_CFG_SPI_PORT_2_DEV_FLAG_SCAN_LUN_EN,
            MPI_CFG_SPI_PORT_2_DEV_FLAG_TAQ_Q_EN,
            MPI_CFG_SPI_PORT_2_DEV_FLAG_WIDE_DIS,
            MPI_CFG_SPI_PORT_2_DEV_FLAG_BOOT_CHOICE,
            MPI_CFG_SPI_PORT_2_PORT_FLAGS_SCAN_HI2LOW,
            MPI_CFG_SPI_PORT_2_PORT_FLAGS_AVOID_RESET,
            MPI_CFG_SPI_PORT_2_PORT_FLAGS_ALT_CHS,
            MPI_CFG_SPI_PORT_2_PORT_FLAGS_TERM_DISABLED,
            MPI_CFG_SPI_PORT_2_PORT_FLAGS_DV_CTL,
            MPI_CFG_SPI_PORT_2_PORT_FLAGS_DV_HOST_BE,
            MPI_CFG_SPI_PORT_2_PORT_FLAGS_DV_HOST_B,
            MPI_CFG_SPI_PORT_2_PORT_FLAGS_DV_HOST_NONE,
            MPI_CFG_SPI_PORT_2_PORT_SET_HOST_ID,
            MPI_CFG_SPI_PORT_2_PORT_SET_INIT_HBA,
            MPI_CFG_SPI_PORT_2_PORT_SET_INIT_HBA_DISABLED,
            MPI_CFG_SPI_PORT_2_PORT_SET_INIT_HBA_BIOS,
            MPI_CFG_SPI_PORT_2_PORT_SET_INIT_HBA_OS,
            MPI_CFG_SPI_PORT_2_PORT_SET_INIT_HBA_BIOS_OS,
            MPI_CFG_SPI_PORT_2_PORT_SET_REMOVABLE,
            MPI_CFG_SPI_PORT_2_PORT_SET_SPINUP_DELAY,
            MPI_CFG_SPI_PORT_2_PORT_SET_SYNC,
            MPI_CFG_SPI_PORT_2_PORT_SET_NEG_SUPPORTED,
            MPI_CFG_SPI_PORT_2_PORT_SET_NEG_NONE,
            MPI_CFG_SPI_PORT_2_PORT_SET_NEG_ALL,
            MPI_CFG_SPI_DEV_0_NEGPARAMS_PACKETIZED,
            MPI_CFG_SPI_DEV_0_NEGPARAMS_DUALXFERS,
            MPI_CFG_SPI_DEV_0_NEGPARAMS_QAS,
            MPI_CFG_SPI_DEV_0_NEGPARAMS_HOLD_MCS,
            MPI_CFG_SPI_DEV_0_NEGPARAMS_WR_FLOW,
            MPI_CFG_SPI_DEV_0_NEGPARAMS_RD_STRM,
            MPI_CFG_SPI_DEV_0_NEGPARAMS_RTI,
            MPI_CFG_SPI_DEV_0_NEGPARAMS_PCOMP_EN,
            MPI_CFG_SPI_DEV_0_NEGPARAMS_IDP_EN,
            MPI_CFG_SPI_DEV_0_NEGPARAMS_WIDTH,
            MPI_CFG_SPI_DEV_0_NEGPARAMS_WIDTH_NARROW,
            MPI_CFG_SPI_DEV_0_NEGPARAMS_WIDTH_WIDE,
            MPI_CFG_SPI_DEV_0_NEGPARAMS_AIP,
            MPI_CFG_SPI_DEV_0_INFO_NEG_OCCURRED,
            MPI_CFG_SPI_DEV_0_INFO_SDTR_REJECTED,
            MPI_CFG_SPI_DEV_0_INFO_WDTR_REJECTED,
            MPI_CFG_SPI_DEV_0_INFO_PPR_REJECTED,
            MPI_CFG_SPI_DEV_1_REQPARAMS_PACKETIZED,
            MPI_CFG_SPI_DEV_1_REQPARAMS_DUALXFERS,
            MPI_CFG_SPI_DEV_1_REQPARAMS_QAS,
            MPI_CFG_SPI_DEV_1_REQPARAMS_HOLD_MCS,
            MPI_CFG_SPI_DEV_1_REQPARAMS_WR_FLOW,
            MPI_CFG_SPI_DEV_1_REQPARAMS_RD_STRM,
            MPI_CFG_SPI_DEV_1_REQPARAMS_RTI,
            MPI_CFG_SPI_DEV_1_REQPARAMS_PCOMP_EN,
            MPI_CFG_SPI_DEV_1_REQPARAMS_IDP_EN,
            MPI_CFG_SPI_DEV_1_REQPARAMS_WIDTH,
            MPI_CFG_SPI_DEV_1_REQPARAMS_WIDTH_NARROW,
            MPI_CFG_SPI_DEV_1_REQPARAMS_WIDTH_WIDE,
            MPI_CFG_SPI_DEV_1_REQPARAMS_AIP,
            MPI_CFG_SPI_DEV_1_CONF_WDTR_DISALLOWED,
            MPI_CFG_SPI_DEV_1_CONF_SDTR_DISALLOWED,
            MPI_CFG_SPI_DEV_1_CONF_EXTPARAMS,
            MPI_CFG_SPI_DEV_1_CONF_FORCE_PPR,
            MPI_CFG_SPI_DEV_2_DV_ISI_ENABLED,
            MPI_CFG_SPI_DEV_2_DV_SECONDARY_DRV_EN,
            MPI_CFG_SPI_DEV_2_DV_SLEW_RATE_CTL,
            MPI_CFG_SPI_DEV_2_DV_PRIMARY_DRV_STRENGTH,
            MPI_CFG_SPI_DEV_2_DV_XCLKH_ST,
            MPI_CFG_SPI_DEV_2_DV_XCLKS_ST,
            MPI_CFG_SPI_DEV_2_DV_XCLKH_DT,
            MPI_CFG_SPI_DEV_2_DV_XCLKS_DT,
            MPI_CFG_SPI_DEV_2_PARITY_PIPE_SELECT,
            MPI_CFG_IOC_1_REPLY_COALESCING,
            MPI_CFG_IOC_1_CTX_REPLY_DISABLE,
            MPI_CFG_IOC_2_CAPABILITIES_IS,
            MPI_CFG_IOC_2_CAPABILITIES_IME,
            MPI_CFG_IOC_2_CAPABILITIES_IM,
            MPI_CFG_IOC_2_CAPABILITIES_SES,
            MPI_CFG_IOC_2_CAPABILITIES_SAFTE,
            MPI_CFG_IOC_2_CAPABILITIES_XCHANNEL,
            MPI_CFG_RAID_TYPE_RAID_IS,
            MPI_CFG_RAID_TYPE_RAID_IME,
            MPI_CFG_RAID_TYPE_RAID_IM,
            MPI_CFG_RAID_TYPE_RAID_5,
            MPI_CFG_RAID_TYPE_RAID_6,
            MPI_CFG_RAID_TYPE_RAID_10,
            MPI_CFG_RAID_TYPE_RAID_50,
            MPI_CFG_RAID_VOL_INACTIVE,
            MPI_CFG_FC_PORT_0_FLAGS_MAP_BY_D_ID,
            MPI_CFG_FC_PORT_0_FLAGS_MAINTAIN_LOGINS,
            MPI_CFG_FC_PORT_0_FLAGS_PLOGI_AFTER_LOGO,
            MPI_CFG_FC_PORT_0_FLAGS_SUPPRESS_PROT_REG,
            MPI_CFG_FC_PORT_0_FLAGS_MASK_RR_TOV_UNITS,
            MPI_CFG_FC_PORT_0_FLAGS_MASK_RR_TOV_UNIT_NONE,
            MPI_CFG_FC_PORT_0_FLAGS_MASK_RR_TOV_UNIT_0_001_SEC,
            MPI_CFG_FC_PORT_0_FLAGS_MASK_RR_TOV_UNIT_0_1_SEC,
            MPI_CFG_FC_PORT_0_FLAGS_MASK_RR_TOV_UNIT_10_SEC,
            MPI_CFG_FC_PORT_0_FLAGS_TGT_LARGE_CDB_EN,
            MPI_CFG_FC_PORT_0_FLAGS_SOFT_ALPA_FALLBACK,
            MPI_CFG_FC_PORT_0_FLAGS_PORT_OFFLINE,
            MPI_CFG_FC_PORT_0_FLAGS_TGT_MODE_OXID,
            MPI_CFG_FC_PORT_0_FLAGS_VERBOSE_RESCAN,
            MPI_CFG_FC_PORT_0_FLAGS_FORCE_NOSEEPROM_WWNS,
            MPI_CFG_FC_PORT_0_FLAGS_IMMEDIATE_ERROR,
            MPI_CFG_FC_PORT_0_FLAGS_EXT_FCP_STATUS_EN,
            MPI_CFG_FC_PORT_0_FLAGS_REQ_PROT_LOG_BUS_ADDR,
            MPI_CFG_FC_PORT_0_FLAGS_REQ_PROT_LAN,
            MPI_CFG_FC_PORT_0_FLAGS_REQ_PROT_TARGET,
            MPI_CFG_FC_PORT_0_FLAGS_REQ_PROT_INITIATOR,
            MPI_CFG_FC_DEV_0_FLAGS_BUSADDR_VALID,
            MPI_CFG_FC_DEV_0_FLAGS_PLOGI_INVALID,
            MPI_CFG_FC_DEV_0_FLAGS_PRLI_INVALID,
            MPI_CFG_RAID_VOL_0_SETTINGS_WRITE_CACHE_EN,
            MPI_CFG_RAID_VOL_0_SETTINGS_OFFLINE_SMART_ERR,
            MPI_CFG_RAID_VOL_0_SETTINGS_OFFLINE_SMART,
            MPI_CFG_RAID_VOL_0_SETTINGS_AUTO_SWAP,
            MPI_CFG_RAID_VOL_0_SETTINGS_HI_PRI_RESYNC,
            MPI_CFG_RAID_VOL_0_SETTINGS_PROD_SUFFIX,
            MPI_CFG_RAID_VOL_0_SETTINGS_FAST_SCRUB,
            MPI_CFG_RAID_VOL_0_SETTINGS_DEFAULTS,
            MPI_CFG_RAID_VOL_0_STATUS_ENABLED,
            MPI_CFG_RAID_VOL_0_STATUS_QUIESCED,
            MPI_CFG_RAID_VOL_0_STATUS_RESYNCING,
            MPI_CFG_RAID_VOL_0_STATUS_ACTIVE,
            MPI_CFG_RAID_VOL_0_STATUS_BADBLOCK_FULL,
            MPI_CFG_RAID_VOL_0_STATE_OPTIMAL,
            MPI_CFG_RAID_VOL_0_STATE_DEGRADED,
            MPI_CFG_RAID_VOL_0_STATE_FAILED,
            MPI_CFG_RAID_VOL_0_STATE_MISSING,
            MPI_CFG_RAID_VOL_0_INACTIVE_UNKNOWN,
            MPI_CFG_RAID_VOL_0_INACTIVE_STALE_META,
            MPI_CFG_RAID_VOL_0_INACTIVE_FOREIGN_VOL,
            MPI_CFG_RAID_VOL_0_INACTIVE_NO_RESOURCES,
            MPI_CFG_RAID_VOL_0_INACTIVE_CLONED_VOL,
            MPI_CFG_RAID_VOL_0_INACTIVE_INSUF_META,
            MPI_CFG_RAID_PHYDISK_0_ENCTYPE_NONE,
            MPI_CFG_RAID_PHYDISK_0_ENCTYPE_SAFTE,
            MPI_CFG_RAID_PHYDISK_0_ENCTYPE_SES,
            MPI_CFG_RAID_PHYDISK_0_STATUS_OUTOFSYNC,
            MPI_CFG_RAID_PHYDISK_0_STATUS_QUIESCED,
            MPI_CFG_RAID_PHYDISK_0_STATE_ONLINE,
            MPI_CFG_RAID_PHYDISK_0_STATE_MISSING,
            MPI_CFG_RAID_PHYDISK_0_STATE_INCOMPAT,
            MPI_CFG_RAID_PHYDISK_0_STATE_FAILED,
            MPI_CFG_RAID_PHYDISK_0_STATE_INIT,
            MPI_CFG_RAID_PHYDISK_0_STATE_OFFLINE,
            MPI_CFG_RAID_PHYDISK_0_STATE_HOSTFAIL,
            MPI_CFG_RAID_PHYDISK_0_STATE_OTHER,
            MPI_CFG_RAID_PHYDISK_PATH_INVALID,
            MPI_CFG_RAID_PHYDISK_PATH_BROKEN,
            MPI_CFG_SAS_DEV_ADDR_NEXT,
            MPI_CFG_SAS_DEV_ADDR_BUS,
            MPI_CFG_SAS_DEV_ADDR_HANDLE,
            MPI_CFG_SAS_DEV_0_DEVINFO_TYPE,
            MPI_CFG_SAS_DEV_0_DEVINFO_TYPE_NONE,
            MPI_CFG_SAS_DEV_0_DEVINFO_TYPE_END,
            MPI_CFG_SAS_DEV_0_DEVINFO_TYPE_EDGE_EXPANDER,
            MPI_CFG_SAS_DEV_0_DEVINFO_TYPE_FANOUT_EXPANDER,
            MPI_CFG_SAS_DEV_0_DEVINFO_SATA_HOST,
            MPI_CFG_SAS_DEV_0_DEVINFO_SMP_INITIATOR,
            MPI_CFG_SAS_DEV_0_DEVINFO_STP_INITIATOR,
            MPI_CFG_SAS_DEV_0_DEVINFO_SSP_INITIATOR,
            MPI_CFG_SAS_DEV_0_DEVINFO_SATA_DEVICE,
            MPI_CFG_SAS_DEV_0_DEVINFO_SMP_TARGET,
            MPI_CFG_SAS_DEV_0_DEVINFO_STP_TARGET,
            MPI_CFG_SAS_DEV_0_DEVINFO_SSP_TARGET,
            MPI_CFG_SAS_DEV_0_DEVINFO_DIRECT_ATTACHED,
            MPI_CFG_SAS_DEV_0_DEVINFO_LSI_DEVICE,
            MPI_CFG_SAS_DEV_0_DEVINFO_ATAPI_DEVICE,
            MPI_CFG_SAS_DEV_0_DEVINFO_SEP_DEVICE,
            MPI_CFG_SAS_DEV_0_FLAGS_DEV_PRESENT,
            MPI_CFG_SAS_DEV_0_FLAGS_DEV_MAPPED,
            MPI_CFG_SAS_DEV_0_FLAGS_DEV_MAPPED_PERSISTENT,
            MPI_CFG_SAS_DEV_0_FLAGS_SATA_PORT_SELECTOR,
            MPI_CFG_SAS_DEV_0_FLAGS_SATA_FUA,
            MPI_CFG_SAS_DEV_0_FLAGS_SATA_NCQ,
            MPI_CFG_SAS_DEV_0_FLAGS_SATA_SMART,
            MPI_CFG_SAS_DEV_0_FLAGS_SATA_LBA48,
            MPI_CFG_SAS_DEV_0_FLAGS_UNSUPPORTED,
            MPI_CFG_SAS_DEV_0_FLAGS_SATA_SETTINGS,
        );
        // MPI_CFG_IOC_2_CAPABILITIES_RAID spans lines; the reftest reads one line per define.
        let mut names = names;
        names.push("MPI_CFG_IOC_2_CAPABILITIES_RAID");
        // The `#if notyet` block of valueless MPI_SCSIIO_ERR_STATUS_* macros is not ported.
        names.extend([
            "MPI_SCSIIO_ERR_STATUS_SUCCESS",
            "MPI_SCSIIO_ERR_STATUS_CHECK_COND",
            "MPI_SCSIIO_ERR_STATUS_BUSY",
            "MPI_SCSIIO_ERR_STATUS_INTERMEDIATE",
            "MPI_SCSIIO_ERR_STATUS_INTERMEDIATE_CONDMET",
            "MPI_SCSIIO_ERR_STATUS_RESERVATION_CONFLICT",
            "MPI_SCSIIO_ERR_STATUS_CMD_TERM",
            "MPI_SCSIIO_ERR_STATUS_TASK_SET_FULL",
            "MPI_SCSIIO_ERR_STATUS_ACA_ACTIVE",
        ]);
        crate::reftest::assert_complete(&defs, "MPI_", &names);
        assert_eq!(
            MPI_CFG_IOC_2_CAPABILITIES_RAID,
            MPI_CFG_IOC_2_CAPABILITIES_IS
                | MPI_CFG_IOC_2_CAPABILITIES_IME
                | MPI_CFG_IOC_2_CAPABILITIES_IM
        );
    }
}
/* </TESTS> */
