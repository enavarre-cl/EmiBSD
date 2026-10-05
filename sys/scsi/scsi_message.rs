/*	$OpenBSD: scsi_message.h,v 1.10 2019/09/27 23:07:42 krw Exp $	*/
/* <LICENSES> */
/* </LICENSES> */

//! `<scsi/scsi_message.h>`: the SCSI messages of the parallel bus (SPI): the one-byte
//! messages, the two-byte queue tag messages, the identify message and the extended
//! messages that negotiate the transfer width (WDTR), the synchronous transfer (SDTR) and
//! the parallel protocol (PPR). Only the parallel SCSI adapters use them (siop(4)).
//!
//! Upstream: sys/scsi/scsi_message.h @ 3ce1f3f79392
//!
//! The original file carries no licence text, only its `$OpenBSD$` line (kept above); every
//! notice in the reference tree is accepted (the user's rule of 2026-10-04).
//!
//! ## Deviations
//! - The function-like macros `IS1BYTEMSG`, `IS2BYTEMSG`, `ISEXTMSG`, `MSG_IDENTIFY` and
//!   `MSG_ISIDENTIFY` are `const fn`s with the macros' names in lower case
//!   ([`is1bytemsg`], [`is2bytemsg`], [`isextmsg`], [`msg_identify`], [`msg_isidentify`]);
//!   `MSG_IDENTIFY`'s `disc` is a `bool`, `MSG_ISIDENTIFY` a `bool` instead of the masked
//!   byte.
//! - Every message and value is a `u8`, the byte that goes on the bus.

/// `IS1BYTEMSG(m)`: a one-byte message.
pub const fn is1bytemsg(m: u8) -> bool {
    (m != 0x01 && m < 0x20) || m >= 0x80
}

/// `IS2BYTEMSG(m)`: a two-byte message.
pub const fn is2bytemsg(m: u8) -> bool {
    (m & 0xf0) == 0x20
}

/// `ISEXTMSG(m)`: an extended message.
pub const fn isextmsg(m: u8) -> bool {
    m == 0x01
}

/* Messages (1 byte) */
/* I/T (M)andatory or (O)ptional */
/// `MSG_CMDCOMPLETE` (M/M).
pub const MSG_CMDCOMPLETE: u8 = 0x00;
/// `MSG_EXTENDED` (O/O).
pub const MSG_EXTENDED: u8 = 0x01;
/// `MSG_SAVEDATAPOINTER` (O/O).
pub const MSG_SAVEDATAPOINTER: u8 = 0x02;
/// `MSG_RESTOREPOINTERS` (O/O).
pub const MSG_RESTOREPOINTERS: u8 = 0x03;
/// `MSG_DISCONNECT` (O/O).
pub const MSG_DISCONNECT: u8 = 0x04;
/// `MSG_INITIATOR_DET_ERR` (M/M).
pub const MSG_INITIATOR_DET_ERR: u8 = 0x05;
/// `MSG_ABORT` (O/M).
pub const MSG_ABORT: u8 = 0x06;
/// `MSG_MESSAGE_REJECT` (M/M).
pub const MSG_MESSAGE_REJECT: u8 = 0x07;
/// `MSG_NOOP` (M/M).
pub const MSG_NOOP: u8 = 0x08;
/// `MSG_PARITY_ERROR` (M/M).
pub const MSG_PARITY_ERROR: u8 = 0x09;
/// `MSG_LINK_CMD_COMPLETE` (O/O).
pub const MSG_LINK_CMD_COMPLETE: u8 = 0x0a;
/// `MSG_LINK_CMD_COMPLETEF` (O/O).
pub const MSG_LINK_CMD_COMPLETEF: u8 = 0x0b;
/// `MSG_BUS_DEV_RESET` (O/M).
pub const MSG_BUS_DEV_RESET: u8 = 0x0c;
/// `MSG_ABORT_TAG` (O/O).
pub const MSG_ABORT_TAG: u8 = 0x0d;
/// `MSG_CLEAR_QUEUE` (O/O).
pub const MSG_CLEAR_QUEUE: u8 = 0x0e;
/// `MSG_INIT_RECOVERY` (O/O).
pub const MSG_INIT_RECOVERY: u8 = 0x0f;
/// `MSG_REL_RECOVERY` (O/O).
pub const MSG_REL_RECOVERY: u8 = 0x10;
/// `MSG_TERM_IO_PROC` (O/O).
pub const MSG_TERM_IO_PROC: u8 = 0x11;
/// `MSG_QAS_REQUEST` (O/O, SPI3).
pub const MSG_QAS_REQUEST: u8 = 0x55;

/* Messages (2 byte) */
/// `MSG_SIMPLE_Q_TAG` (O/O).
pub const MSG_SIMPLE_Q_TAG: u8 = 0x20;
/// `MSG_HEAD_OF_Q_TAG` (O/O).
pub const MSG_HEAD_OF_Q_TAG: u8 = 0x21;
/// `MSG_ORDERED_Q_TAG` (O/O).
pub const MSG_ORDERED_Q_TAG: u8 = 0x22;
/// `MSG_IGN_WIDE_RESIDUE` (O/O).
pub const MSG_IGN_WIDE_RESIDUE: u8 = 0x23;

/* Identify message (M/M) */
/// `MSG_IDENTIFYFLAG`.
pub const MSG_IDENTIFYFLAG: u8 = 0x80;
/// `MSG_IDENTIFY_DISCFLAG`.
pub const MSG_IDENTIFY_DISCFLAG: u8 = 0x40;

/// `MSG_IDENTIFY(lun, disc)`: the identify message for `lun`, with the disconnect
/// privilege when `disc`.
pub const fn msg_identify(lun: u8, disc: bool) -> u8 {
    (if disc { 0xc0 } else { MSG_IDENTIFYFLAG }) | lun
}

/// `MSG_ISIDENTIFY(m)`: an identify message.
pub const fn msg_isidentify(m: u8) -> bool {
    m & MSG_IDENTIFYFLAG != 0
}

/// `MSG_IDENTIFY_LUNMASK`.
pub const MSG_IDENTIFY_LUNMASK: u8 = 0x01f;

/* Extended messages (opcode and length) */
/// `MSG_EXT_SDTR`.
pub const MSG_EXT_SDTR: u8 = 0x01;
/// `MSG_EXT_SDTR_LEN`.
pub const MSG_EXT_SDTR_LEN: u8 = 0x03;

/// `MSG_EXT_WDTR`.
pub const MSG_EXT_WDTR: u8 = 0x03;
/// `MSG_EXT_WDTR_LEN`.
pub const MSG_EXT_WDTR_LEN: u8 = 0x02;

/// `MSG_EXT_WDTR_BUS_8_BIT`.
pub const MSG_EXT_WDTR_BUS_8_BIT: u8 = 0x00;
/// `MSG_EXT_WDTR_BUS_16_BIT`.
pub const MSG_EXT_WDTR_BUS_16_BIT: u8 = 0x01;
/// `MSG_EXT_WDTR_BUS_32_BIT`.
pub const MSG_EXT_WDTR_BUS_32_BIT: u8 = 0x02;

/// `MSG_EXT_PPR`.
pub const MSG_EXT_PPR: u8 = 0x04;
/// `MSG_EXT_PPR_LEN`.
pub const MSG_EXT_PPR_LEN: u8 = 0x06;

/// `MSG_EXT_PPR_PCOMP_EN`.
pub const MSG_EXT_PPR_PCOMP_EN: u8 = 0x80;
/// `MSG_EXT_PPR_RTI`.
pub const MSG_EXT_PPR_RTI: u8 = 0x40;
/// `MSG_EXT_PPR_RD_STRM`.
pub const MSG_EXT_PPR_RD_STRM: u8 = 0x20;
/// `MSG_EXT_PPR_WR_FLOW`.
pub const MSG_EXT_PPR_WR_FLOW: u8 = 0x10;
/// `MSG_EXT_PPR_HOLD_MCS`.
pub const MSG_EXT_PPR_HOLD_MCS: u8 = 0x08;
/// `MSG_EXT_PPR_PROT_QAS`.
pub const MSG_EXT_PPR_PROT_QAS: u8 = 0x04;
/// `MSG_EXT_PPR_PROT_DT`.
pub const MSG_EXT_PPR_PROT_DT: u8 = 0x02;
/// `MSG_EXT_PPR_PROT_IUS`.
pub const MSG_EXT_PPR_PROT_IUS: u8 = 0x01;

#[cfg(test)]
mod tests;
