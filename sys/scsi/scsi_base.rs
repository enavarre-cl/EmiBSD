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
 * Copyright (c) 1994, 1995, 1997 Charles M. Hannum.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by Charles M. Hannum.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

/*
 * Originally written by Julian Elischer (julian@dialix.oz.au)
 * Detailed SCSI error printing Copyright 1997 by Matthew Jacob.
 */
/* </LICENSES> */

/* <CODE> */
//! The SCSI midlayer's common code: the I/O pools that meter an adapter's openings and the
//! handlers that wait on them, the allocation of transfers (`scsi_xs_get`/`scsi_xs_put`),
//! their execution (`scsi_xs_exec`, `scsi_done`, `scsi_xs_sync`), the error and sense
//! interpretation every device driver shares, and the commands every SCSI device answers
//! (TEST UNIT READY, INQUIRY, MODE SENSE/SELECT, PREVENT ALLOW, REPORT LUNS).
//!
//! Upstream: sys/scsi/scsi_base.c @ 3ce1f3f79392
//!
//! How a transfer flows: a device driver either asks for one synchronously with
//! [`scsi_xs_get`] (which may sleep for an opening of its link and of the adapter's pool) or
//! queues an [`ScsiXshandler`] with [`scsi_xsh_add`], whose handler later gets a ready
//! transfer. It fills `cmd`/`cmdlen` and the data ([`ScsiXfer::set_data`]), then either sets
//! `done` and calls [`scsi_xs_exec`], or calls [`scsi_xs_sync`], which sleeps (or, with
//! `SCSI_NOSLEEP`, makes the adapter poll) until the adapter's `scsi_cmd` has ended the
//! transfer with [`scsi_done`], retries while the error says `ERESTART`, and returns the
//! errno. [`scsi_xs_put`] gives the transfer, the opening and the link's slot back.
//!
//! ## Deviations
//! - `scsi_cmd_rw_decode` returns the block number and count as a tuple, where the C writes
//!   them through two pointers.
//! - `SCSIDEBUG` is not configured (`scsi_debug.rs` has the `SDEV_DB*` bits and the empty
//!   `SC_DEBUG*` macros). The `SC_DEBUG`, `SC_DEBUG_SENSE` and `#ifdef SCSIDEBUG` sites are
//!   comments, and the debug-only functions
//!   at the end of the C file (`scsi_show_sense`, `scsi_show_xs`, `scsi_show_mem`,
//!   `scsi_show_flags`, `scsi_show_inquiry_header`, `scsi_show_inquiry_match` and the
//!   `flagnames`/`quirknames`/`devicetypenames`/`scsidebug_*` tables) are not ported.
//!   `SCSI_DELAY` is not configured either (a comment in `scsi_init`). `SCSITERSE` is the
//!   cargo feature `scsiterse`.
//! - Functions returning 0 or an errno return `Result<(), Errno>`; `Err(ERESTART)` from an
//!   `interpret_sense` or [`scsi_delay`] means "retry", as the C's `ERESTART`.
//! - Buffers the C passes as `void *` plus a length (`scsi_inquire_vpd`, `scsi_mode_select`,
//!   `scsi_copy_internal_data`) are byte slices; typed ones are references to the
//!   [`ScsiWire`] structures. `scsi_mode_select`/`_big` clamp the length the header claims
//!   to the slice they are given (the C trusts it).
//! - `scsi_do_mode_sense` returns the page as `Option<usize>`, the offset of the page in
//!   `buf.buf` (the C's `void **page_data`, `NULL` being `None`), and `big` as a `bool`, in
//!   the `Ok` value; on an error the C's `*page_data` and `*big` are always `NULL` and 0.
//!   `scsi_mode_sense_page` and `scsi_mode_sense_big_page` likewise return an offset, and
//!   also `None` when the header puts the page past the end of the buffer (where the C
//!   would read past the union).
//! - `scsi_decode_sense` returns its 132-byte buffer by value instead of a `static char
//!   rqsbuf[132]`.
//! - The `scsi_xshandler` and `scsi_io_mover` that `scsi_xs_get` and `scsi_io_get` keep on
//!   their stack are linked into the run queues with the queues' `unsafe` mutators, as the
//!   futex waiters are (`docs/C_TO_RUST.md`); the public [`scsi_ioh_add`] and
//!   [`scsi_xsh_add`] take `&'static` handlers.
//! - `scsi_link_shutdown` tells the transfer handlers on the pool's queue by the handler's
//!   `is_xsh` flag before casting them (see `scsiconf.rs`).
//! - `scsi_done` and `scsi_xsh_ioh` panic when the transfer has no `done`, the xshandler no
//!   `handler`, or the handler gets no opening, and `scsi_xs_put` when the transfer has no
//!   opening, where the C would call through or pass on a NULL pointer.
//! - The pools are `pool_init`ed with `size_of::<ScsiXfer>` and `size_of::<ScsiPlug>`;
//!   each item is written with its Rust constructor after `pool_get`, as `buf_get` does.

use core::cell::Cell;
use core::ffi::c_void;
use core::fmt;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, Ordering};

use libkern::strlcpy;

use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_synch::{msleep_nsec, nowake, tsleep_nsec, wakeup_one};
use crate::kern::kern_task::{SYSTQ, task_add};
use crate::kern::subr_pool::{pool_get, pool_init, pool_prime, pool_put, pool_setlowat};
use crate::kern::subr_prf::{Str, panic, snprintf};
use crate::kprintf;
use crate::machine::cpu::delay;
use crate::machine::intr::IPL_BIO;
use crate::scsi::scsi_all::*;
use crate::scsi::scsi_disk::*;
use crate::scsi::scsiconf::*;
use crate::sys::device::Device;
use crate::sys::errno::Errno::{self, *};
use crate::sys::mutex::Mutex;
use crate::sys::param::{PCATCH, PRIBIO};
use crate::sys::pool::{PR_NOWAIT, PR_WAITOK, PR_ZERO, Pool};
use crate::sys::systm::{INFSLP, kernel_lock, kernel_unlock};
use crate::sys::task::Task;
use crate::sys::time::sec_to_nsec;

/// `DECODE_SENSE_KEY`: `scsi_decode_sense` gives the sense key's name.
const DECODE_SENSE_KEY: i32 = 1;
/// `DECODE_ASC_ASCQ`: the additional sense code's description.
const DECODE_ASC_ASCQ: i32 = 2;
/// `DECODE_SKSV`: the sense key specific information.
const DECODE_SKSV: i32 = 3;

/// `RUNQ_IDLE`: the handler is on no queue.
const RUNQ_IDLE: u32 = 0;
/// `RUNQ_LINKQ`: the transfer handler is on its link's queue.
const RUNQ_LINKQ: u32 = 1;
/// `RUNQ_POOLQ`: the handler is on its pool's queue.
const RUNQ_POOLQ: u32 = 2;

/// `sense_keys[]`: the names of the sense keys.
const SENSE_KEYS: [&str; 16] = [
    "No Additional Sense",
    "Soft Error",
    "Not Ready",
    "Media Error",
    "Hardware Error",
    "Illegal Request",
    "Unit Attention",
    "Write Protected",
    "Blank Check",
    "Vendor Unique",
    "Copy Aborted",
    "Aborted Command",
    "Equal Error",
    "Volume Overflow",
    "Miscompare Error",
    "Reserved",
];

/// `adesc[]`: the fixed descriptions of the additional sense codes and their qualifiers
/// (www.t10.org/lists/asc-num.txt as of 11/15/10). The C's `{ 0x00, 0x00, NULL }` terminator
/// is the end of the slice.
#[cfg(not(feature = "scsiterse"))]
const ADESC: &[(u8, u8, &str)] = &[
    (0x00, 0x00, "No Additional Sense Information"),
    (0x00, 0x01, "Filemark Detected"),
    (0x00, 0x02, "End-Of-Partition/Medium Detected"),
    (0x00, 0x03, "Setmark Detected"),
    (0x00, 0x04, "Beginning-Of-Partition/Medium Detected"),
    (0x00, 0x05, "End-Of-Data Detected"),
    (0x00, 0x06, "I/O Process Terminated"),
    (0x00, 0x11, "Audio Play Operation In Progress"),
    (0x00, 0x12, "Audio Play Operation Paused"),
    (0x00, 0x13, "Audio Play Operation Successfully Completed"),
    (0x00, 0x14, "Audio Play Operation Stopped Due to Error"),
    (0x00, 0x15, "No Current Audio Status To Return"),
    (0x00, 0x16, "Operation In Progress"),
    (0x00, 0x17, "Cleaning Requested"),
    (0x00, 0x18, "Erase Operation In Progress"),
    (0x00, 0x19, "Locate Operation In Progress"),
    (0x00, 0x1a, "Rewind Operation In Progress"),
    (0x00, 0x1b, "Set Capacity Operation In Progress"),
    (0x00, 0x1c, "Verify Operation In Progress"),
    (0x01, 0x00, "No Index/Sector Signal"),
    (0x02, 0x00, "No Seek Complete"),
    (0x03, 0x00, "Peripheral Device Write Fault"),
    (0x03, 0x01, "No Write Current"),
    (0x03, 0x02, "Excessive Write Errors"),
    (0x04, 0x00, "Logical Unit Not Ready, Cause Not Reportable"),
    (0x04, 0x01, "Logical Unit Is in Process Of Becoming Ready"),
    (
        0x04,
        0x02,
        "Logical Unit Not Ready, Initialization Command Required",
    ),
    (
        0x04,
        0x03,
        "Logical Unit Not Ready, Manual Intervention Required",
    ),
    (0x04, 0x04, "Logical Unit Not Ready, Format In Progress"),
    (0x04, 0x05, "Logical Unit Not Ready, Rebuild In Progress"),
    (
        0x04,
        0x06,
        "Logical Unit Not Ready, Recalculation In Progress",
    ),
    (0x04, 0x07, "Logical Unit Not Ready, Operation In Progress"),
    (0x04, 0x08, "Logical Unit Not Ready, Long Write In Progress"),
    (0x04, 0x09, "Logical Unit Not Ready, Self-Test In Progress"),
    (
        0x04,
        0x0a,
        "Logical Unit Not Accessible, Asymmetric Access State Transition",
    ),
    (
        0x04,
        0x0b,
        "Logical Unit Not Accessible, Target Port In Standby State",
    ),
    (
        0x04,
        0x0c,
        "Logical Unit Not Accessible, Target Port In Unavailable State",
    ),
    (
        0x04,
        0x0d,
        "Logical Unit Not Ready, Structure Check Required",
    ),
    (
        0x04,
        0x10,
        "Logical Unit Not Ready, Auxiliary Memory Not Accessible",
    ),
    (
        0x04,
        0x11,
        "Logical Unit Not Ready, Notify (Enable Spinup) Required",
    ),
    (0x04, 0x12, "Logical Unit Not Ready, Offline"),
    (
        0x04,
        0x13,
        "Logical Unit Not Ready, SA Creation In Progress",
    ),
    (
        0x04,
        0x14,
        "Logical Unit Not Ready, Space Allocation In Progress",
    ),
    (0x04, 0x15, "Logical Unit Not Ready, Robotics Disabled"),
    (0x04, 0x16, "Logical Unit Not Ready, Configuration Required"),
    (0x04, 0x17, "Logical Unit Not Ready, Calibration Required"),
    (0x04, 0x18, "Logical Unit Not Ready, A Door Is Open"),
    (
        0x04,
        0x19,
        "Logical Unit Not Ready, Operating In Sequential Mode",
    ),
    (
        0x04,
        0x1a,
        "Logical Unit Not Ready, Start Stop Unit Command In Progress",
    ),
    (0x05, 0x00, "Logical Unit Does Not Respond To Selection"),
    (0x06, 0x00, "No Reference Position Found"),
    (0x07, 0x00, "Multiple Peripheral Devices Selected"),
    (0x08, 0x00, "Logical Unit Communication Failure"),
    (0x08, 0x01, "Logical Unit Communication Timeout"),
    (0x08, 0x02, "Logical Unit Communication Parity Error"),
    (
        0x08,
        0x03,
        "Logical Unit Communication CRC Error (ULTRA-DMA/32)",
    ),
    (0x08, 0x04, "Unreachable Copy Target"),
    (0x09, 0x00, "Track Following Error"),
    (0x09, 0x01, "Tracking Servo Failure"),
    (0x09, 0x02, "Focus Servo Failure"),
    (0x09, 0x03, "Spindle Servo Failure"),
    (0x09, 0x04, "Head Select Fault"),
    (0x0a, 0x00, "Error Log Overflow"),
    (0x0b, 0x00, "Warning"),
    (0x0b, 0x01, "Warning - Specified Temperature Exceeded"),
    (0x0b, 0x02, "Warning - Enclosure Degraded"),
    (0x0b, 0x03, "Warning - Background Self-Test Failed"),
    (
        0x0b,
        0x04,
        "Warning - Background Pre-Scan Detected Medium Error",
    ),
    (
        0x0b,
        0x05,
        "Warning - Background Medium Scan Detected Medium Error",
    ),
    (0x0b, 0x06, "Warning - Non-Volatile Cache Now Volatile"),
    (0x0b, 0x07, "Warning - Degraded Power To Non-Volatile Cache"),
    (0x0b, 0x08, "Warning - Power Loss Expected"),
    (0x0c, 0x00, "Write Error"),
    (0x0c, 0x01, "Write Error Recovered with Auto Reallocation"),
    (0x0c, 0x02, "Write Error - Auto Reallocate Failed"),
    (0x0c, 0x03, "Write Error - Recommend Reassignment"),
    (0x0c, 0x04, "Compression Check Miscompare Error"),
    (0x0c, 0x05, "Data Expansion Occurred During Compression"),
    (0x0c, 0x06, "Block Not Compressible"),
    (0x0c, 0x07, "Write Error - Recovery Needed"),
    (0x0c, 0x08, "Write Error - Recovery Failed"),
    (0x0c, 0x09, "Write Error - Loss Of Streaming"),
    (0x0c, 0x0a, "Write Error - Padding Blocks Added"),
    (0x0c, 0x0b, "Auxiliary Memory Write Error"),
    (0x0c, 0x0c, "Write Error - Unexpected Unsolicited Data"),
    (0x0c, 0x0d, "Write Error - Not Enough Unsolicited Data"),
    (0x0c, 0x0f, "Defects In Error Window"),
    (
        0x0d,
        0x00,
        "Error Detected By Third Party Temporary Initiator",
    ),
    (0x0d, 0x01, "Third Party Device Failure"),
    (0x0d, 0x02, "Copy Target Device Not Reachable"),
    (0x0d, 0x03, "Incorrect Copy Target Device Type"),
    (0x0d, 0x04, "Copy Target Device Data Underrun"),
    (0x0d, 0x05, "Copy Target Device Data Overrun"),
    (0x0e, 0x00, "Invalid Information Unit"),
    (0x0e, 0x01, "Information Unit Too Short"),
    (0x0e, 0x02, "Information Unit Too Long"),
    (0x10, 0x00, "ID CRC Or ECC Error"),
    (0x10, 0x01, "Logical Block Guard Check Failed"),
    (0x10, 0x02, "Logical Block Application Tag Check Failed"),
    (0x10, 0x03, "Logical Block Reference Tag Check Failed"),
    (
        0x10,
        0x04,
        "Logical Block Protection Error On Recover Buffered Data",
    ),
    (0x10, 0x05, "Logical Block Protection Method Error"),
    (0x11, 0x00, "Unrecovered Read Error"),
    (0x11, 0x01, "Read Retries Exhausted"),
    (0x11, 0x02, "Error Too Long To Correct"),
    (0x11, 0x03, "Multiple Read Errors"),
    (
        0x11,
        0x04,
        "Unrecovered Read Error - Auto Reallocate Failed",
    ),
    (0x11, 0x05, "L-EC Uncorrectable Error"),
    (0x11, 0x06, "CIRC Unrecovered Error"),
    (0x11, 0x07, "Data Resynchronization Error"),
    (0x11, 0x08, "Incomplete Block Read"),
    (0x11, 0x09, "No Gap Found"),
    (0x11, 0x0a, "Miscorrected Error"),
    (
        0x11,
        0x0b,
        "Uncorrected Read Error - Recommend Reassignment",
    ),
    (
        0x11,
        0x0c,
        "Uncorrected Read Error - Recommend Rewrite The Data",
    ),
    (0x11, 0x0d, "De-Compression CRC Error"),
    (0x11, 0x0e, "Cannot Decompress Using Declared Algorithm"),
    (0x11, 0x0f, "Error Reading UPC/EAN Number"),
    (0x11, 0x10, "Error Reading ISRC Number"),
    (0x11, 0x11, "Read Error - Loss Of Streaming"),
    (0x11, 0x12, "Auxiliary Memory Read Error"),
    (0x11, 0x13, "Read Error - Failed Retransmission Request"),
    (
        0x11,
        0x14,
        "Read Error - LBA Marked Bad By Application Client",
    ),
    (0x12, 0x00, "Address Mark Not Found for ID Field"),
    (0x13, 0x00, "Address Mark Not Found for Data Field"),
    (0x14, 0x00, "Recorded Entity Not Found"),
    (0x14, 0x01, "Record Not Found"),
    (0x14, 0x02, "Filemark or Setmark Not Found"),
    (0x14, 0x03, "End-Of-Data Not Found"),
    (0x14, 0x04, "Block Sequence Error"),
    (0x14, 0x05, "Record Not Found - Recommend Reassignment"),
    (0x14, 0x06, "Record Not Found - Data Auto-Reallocated"),
    (0x14, 0x07, "Locate Operation Failure"),
    (0x15, 0x00, "Random Positioning Error"),
    (0x15, 0x01, "Mechanical Positioning Error"),
    (0x15, 0x02, "Positioning Error Detected By Read of Medium"),
    (0x16, 0x00, "Data Synchronization Mark Error"),
    (0x16, 0x01, "Data Sync Error - Data Rewritten"),
    (0x16, 0x02, "Data Sync Error - Recommend Rewrite"),
    (0x16, 0x03, "Data Sync Error - Data Auto-Reallocated"),
    (0x16, 0x04, "Data Sync Error - Recommend Reassignment"),
    (
        0x17,
        0x00,
        "Recovered Data With No Error Correction Applied",
    ),
    (0x17, 0x01, "Recovered Data With Retries"),
    (0x17, 0x02, "Recovered Data With Positive Head Offset"),
    (0x17, 0x03, "Recovered Data With Negative Head Offset"),
    (
        0x17,
        0x04,
        "Recovered Data With Retries and/or CIRC Applied",
    ),
    (0x17, 0x05, "Recovered Data Using Previous Sector ID"),
    (
        0x17,
        0x06,
        "Recovered Data Without ECC - Data Auto-Reallocated",
    ),
    (
        0x17,
        0x07,
        "Recovered Data Without ECC - Recommend Reassignment",
    ),
    (0x17, 0x08, "Recovered Data Without ECC - Recommend Rewrite"),
    (0x17, 0x09, "Recovered Data Without ECC - Data Rewritten"),
    (0x18, 0x00, "Recovered Data With Error Correction Applied"),
    (
        0x18,
        0x01,
        "Recovered Data With Error Correction & Retries Applied",
    ),
    (0x18, 0x02, "Recovered Data - Data Auto-Reallocated"),
    (0x18, 0x03, "Recovered Data With CIRC"),
    (0x18, 0x04, "Recovered Data With L-EC"),
    (0x18, 0x05, "Recovered Data - Recommend Reassignment"),
    (0x18, 0x06, "Recovered Data - Recommend Rewrite"),
    (0x18, 0x07, "Recovered Data With ECC - Data Rewritten"),
    (0x18, 0x08, "Recovered Data With Linking"),
    (0x19, 0x00, "Defect List Error"),
    (0x19, 0x01, "Defect List Not Available"),
    (0x19, 0x02, "Defect List Error in Primary List"),
    (0x19, 0x03, "Defect List Error in Grown List"),
    (0x1a, 0x00, "Parameter List Length Error"),
    (0x1b, 0x00, "Synchronous Data Transfer Error"),
    (0x1c, 0x00, "Defect List Not Found"),
    (0x1c, 0x01, "Primary Defect List Not Found"),
    (0x1c, 0x02, "Grown Defect List Not Found"),
    (0x1d, 0x00, "Miscompare During Verify Operation"),
    (0x1d, 0x01, "Miscompare Verify Of Unmapped Lba"),
    (0x1e, 0x00, "Recovered ID with ECC"),
    (0x1f, 0x00, "Partial Defect List Transfer"),
    (0x20, 0x00, "Invalid Command Operation Code"),
    (0x20, 0x01, "Access Denied - Initiator Pending-Enrolled"),
    (0x20, 0x02, "Access Denied - No Access rights"),
    (0x20, 0x03, "Access Denied - Invalid Mgmt ID Key"),
    (0x20, 0x04, "Illegal Command While In Write Capable State"),
    (0x20, 0x05, "Obsolete"),
    (0x20, 0x06, "Illegal Command While In Explicit Address Mode"),
    (0x20, 0x07, "Illegal Command While In Implicit Address Mode"),
    (0x20, 0x08, "Access Denied - Enrollment Conflict"),
    (0x20, 0x09, "Access Denied - Invalid LU Identifier"),
    (0x20, 0x0a, "Access Denied - Invalid Proxy Token"),
    (0x20, 0x0b, "Access Denied - ACL LUN Conflict"),
    (0x20, 0x0c, "Illegal Command When Not In Append-Only Mode"),
    (0x21, 0x00, "Logical Block Address Out of Range"),
    (0x21, 0x01, "Invalid Element Address"),
    (0x21, 0x02, "Invalid Address For Write"),
    (0x21, 0x03, "Invalid Write Crossing Layer Jump"),
    (
        0x22,
        0x00,
        "Illegal Function (Should 20 00, 24 00, or 26 00)",
    ),
    (0x24, 0x00, "Illegal Field in CDB"),
    (0x24, 0x01, "CDB Decryption Error"),
    (0x24, 0x02, "Obsolete"),
    (0x24, 0x03, "Obsolete"),
    (0x24, 0x04, "Security Audit Value Frozen"),
    (0x24, 0x05, "Security Working Key Frozen"),
    (0x24, 0x06, "Nonce Not Unique"),
    (0x24, 0x07, "Nonce Timestamp Out Of Range"),
    (0x24, 0x08, "Invalid XCDB"),
    (0x25, 0x00, "Logical Unit Not Supported"),
    (0x26, 0x00, "Invalid Field In Parameter List"),
    (0x26, 0x01, "Parameter Not Supported"),
    (0x26, 0x02, "Parameter Value Invalid"),
    (0x26, 0x03, "Threshold Parameters Not Supported"),
    (0x26, 0x04, "Invalid Release Of Persistent Reservation"),
    (0x26, 0x05, "Data Decryption Error"),
    (0x26, 0x06, "Too Many Target Descriptors"),
    (0x26, 0x07, "Unsupported Target Descriptor Type Code"),
    (0x26, 0x08, "Too Many Segment Descriptors"),
    (0x26, 0x09, "Unsupported Segment Descriptor Type Code"),
    (0x26, 0x0a, "Unexpected Inexact Segment"),
    (0x26, 0x0b, "Inline Data Length Exceeded"),
    (
        0x26,
        0x0c,
        "Invalid Operation For Copy Source Or Destination",
    ),
    (0x26, 0x0d, "Copy Segment Granularity Violation"),
    (0x26, 0x0e, "Invalid Parameter While Port Is Enabled"),
    (0x26, 0x0f, "Invalid Data-Out Buffer Integrity Check Value"),
    (0x26, 0x10, "Data Decryption Key Fail Limit Reached"),
    (0x26, 0x11, "Incomplete Key-Associated Data Set"),
    (0x26, 0x12, "Vendor Specific Key Reference Not Found"),
    (0x27, 0x00, "Write Protected"),
    (0x27, 0x01, "Hardware Write Protected"),
    (0x27, 0x02, "Logical Unit Software Write Protected"),
    (0x27, 0x03, "Associated Write Protect"),
    (0x27, 0x04, "Persistent Write Protect"),
    (0x27, 0x05, "Permanent Write Protect"),
    (0x27, 0x06, "Conditional Write Protect"),
    (0x27, 0x07, "Space Allocation Failed Write Protect"),
    (
        0x28,
        0x00,
        "Not Ready To Ready Transition (Medium May Have Changed)",
    ),
    (0x28, 0x01, "Import Or Export Element Accessed"),
    (0x28, 0x02, "Format-Layer May Have Changed"),
    (0x28, 0x03, "Import/Export Element Accessed, Medium Changed"),
    (0x29, 0x00, "Power On, Reset, or Bus Device Reset Occurred"),
    (0x29, 0x01, "Power On Occurred"),
    (0x29, 0x02, "SCSI Bus Reset Occurred"),
    (0x29, 0x03, "Bus Device Reset Function Occurred"),
    (0x29, 0x04, "Device Internal Reset"),
    (0x29, 0x05, "Transceiver Mode Changed to Single Ended"),
    (0x29, 0x06, "Transceiver Mode Changed to LVD"),
    (0x29, 0x07, "I_T Nexus Loss Occurred"),
    (0x2a, 0x00, "Parameters Changed"),
    (0x2a, 0x01, "Mode Parameters Changed"),
    (0x2a, 0x02, "Log Parameters Changed"),
    (0x2a, 0x03, "Reservations Preempted"),
    (0x2a, 0x04, "Reservations Released"),
    (0x2a, 0x05, "Registrations Preempted"),
    (0x2a, 0x06, "Asymmetric Access State Changed"),
    (
        0x2a,
        0x07,
        "Implicit Asymmetric Access State Transition Failed",
    ),
    (0x2a, 0x08, "Priority Changed"),
    (0x2a, 0x09, "Capacity Data Has Changed"),
    (0x2a, 0x0a, "Error History I_T Nexus Cleared"),
    (0x2a, 0x0b, "Error History Snapshot Released"),
    (0x2a, 0x0c, "Error Recovery Attributes Have Changed"),
    (0x2a, 0x0d, "Data Encryption Capabilities Changed"),
    (0x2a, 0x10, "Timestamp Changed"),
    (
        0x2a,
        0x11,
        "Data Encryption Parameters Changed By Another I_T Nexus",
    ),
    (
        0x2a,
        0x12,
        "Data Encryption Parameters Changed By Vendor Specific Event",
    ),
    (
        0x2a,
        0x13,
        "Data Encryption Key Instance Counter Has Changed",
    ),
    (0x2a, 0x14, "SA Creation Capabilities Data Has Changed"),
    (
        0x2b,
        0x00,
        "Copy Cannot Execute Since Host Cannot Disconnect",
    ),
    (0x2c, 0x00, "Command Sequence Error"),
    (0x2c, 0x01, "Too Many Windows Specified"),
    (0x2c, 0x02, "Invalid Combination of Windows Specified"),
    (0x2c, 0x03, "Current Program Area Is Not Empty"),
    (0x2c, 0x04, "Current Program Area Is Empty"),
    (0x2c, 0x05, "Illegal Power Condition Request"),
    (0x2c, 0x06, "Persistent Prevent Conflict"),
    (0x2c, 0x07, "Previous Busy Status"),
    (0x2c, 0x08, "Previous Task Set Full Status"),
    (0x2c, 0x09, "Previous Reservation Conflict Status"),
    (0x2c, 0x0a, "Partition Or Collection Contains User Objects"),
    (0x2c, 0x0b, "Not Reserved"),
    (0x2c, 0x0c, "ORWrite Generation Does Not Match"),
    (0x2d, 0x00, "Overwrite Error On Update In Place"),
    (0x2e, 0x00, "Insufficient Time For Operation"),
    (0x2f, 0x00, "Commands Cleared By Another Initiator"),
    (0x2f, 0x01, "Commands Cleared By Power Loss Notification"),
    (0x2f, 0x02, "Commands Cleared By Device Server"),
    (0x30, 0x00, "Incompatible Medium Installed"),
    (0x30, 0x01, "Cannot Read Medium - Unknown Format"),
    (0x30, 0x02, "Cannot Read Medium - Incompatible Format"),
    (0x30, 0x03, "Cleaning Cartridge Installed"),
    (0x30, 0x04, "Cannot Write Medium - Unknown Format"),
    (0x30, 0x05, "Cannot Write Medium - Incompatible Format"),
    (0x30, 0x06, "Cannot Format Medium - Incompatible Medium"),
    (0x30, 0x07, "Cleaning Failure"),
    (0x30, 0x08, "Cannot Write - Application Code Mismatch"),
    (0x30, 0x09, "Current Session Not Fixated For Append"),
    (0x30, 0x0a, "Cleaning Request Rejected"),
    (0x30, 0x10, "Medium Not Formatted"),
    (0x30, 0x11, "Incompatible Volume Type"),
    (0x30, 0x12, "Incompatible Volume Qualifier"),
    (0x30, 0x13, "Cleaning Volume Expired"),
    (0x31, 0x00, "Medium Format Corrupted"),
    (0x31, 0x01, "Format Command Failed"),
    (0x31, 0x02, "Zoned Formatting Failed Due To Spare Linking"),
    (0x32, 0x00, "No Defect Spare Location Available"),
    (0x32, 0x01, "Defect List Update Failure"),
    (0x33, 0x00, "Tape Length Error"),
    (0x34, 0x00, "Enclosure Failure"),
    (0x35, 0x00, "Enclosure Services Failure"),
    (0x35, 0x01, "Unsupported Enclosure Function"),
    (0x35, 0x02, "Enclosure Services Unavailable"),
    (0x35, 0x03, "Enclosure Services Transfer Failure"),
    (0x35, 0x04, "Enclosure Services Transfer Refused"),
    (0x36, 0x00, "Ribbon, Ink, or Toner Failure"),
    (0x37, 0x00, "Rounded Parameter"),
    (0x38, 0x00, "Event Status Notification"),
    (0x38, 0x02, "ESN - Power Management Class Event"),
    (0x38, 0x04, "ESN - Media Class Event"),
    (0x38, 0x06, "ESN - Device Busy Class Event"),
    (0x39, 0x00, "Saving Parameters Not Supported"),
    (0x3a, 0x00, "Medium Not Present"),
    (0x3a, 0x01, "Medium Not Present - Tray Closed"),
    (0x3a, 0x02, "Medium Not Present - Tray Open"),
    (0x3a, 0x03, "Medium Not Present - Loadable"),
    (
        0x3a,
        0x04,
        "Medium Not Present - Medium Auxiliary Memory Accessible",
    ),
    (0x3b, 0x00, "Sequential Positioning Error"),
    (0x3b, 0x01, "Tape Position Error At Beginning-of-Medium"),
    (0x3b, 0x02, "Tape Position Error At End-of-Medium"),
    (
        0x3b,
        0x03,
        "Tape or Electronic Vertical Forms Unit Not Ready",
    ),
    (0x3b, 0x04, "Slew Failure"),
    (0x3b, 0x05, "Paper Jam"),
    (0x3b, 0x06, "Failed To Sense Top-Of-Form"),
    (0x3b, 0x07, "Failed To Sense Bottom-Of-Form"),
    (0x3b, 0x08, "Reposition Error"),
    (0x3b, 0x09, "Read Past End Of Medium"),
    (0x3b, 0x0a, "Read Past Beginning Of Medium"),
    (0x3b, 0x0b, "Position Past End Of Medium"),
    (0x3b, 0x0c, "Position Past Beginning Of Medium"),
    (0x3b, 0x0d, "Medium Destination Element Full"),
    (0x3b, 0x0e, "Medium Source Element Empty"),
    (0x3b, 0x0f, "End Of Medium Reached"),
    (0x3b, 0x11, "Medium Magazine Not Accessible"),
    (0x3b, 0x12, "Medium Magazine Removed"),
    (0x3b, 0x13, "Medium Magazine Inserted"),
    (0x3b, 0x14, "Medium Magazine Locked"),
    (0x3b, 0x15, "Medium Magazine Unlocked"),
    (0x3b, 0x16, "Mechanical Positioning Or Changer Error"),
    (0x3b, 0x17, "Read Past End Of User Object"),
    (0x3b, 0x18, "Element Disabled"),
    (0x3b, 0x19, "Element Enabled"),
    (0x3b, 0x1a, "Data Transfer Device Removed"),
    (0x3b, 0x1b, "Data Transfer Device Inserted"),
    (0x3d, 0x00, "Invalid Bits In IDENTIFY Message"),
    (0x3e, 0x00, "Logical Unit Has Not Self-Configured Yet"),
    (0x3e, 0x01, "Logical Unit Failure"),
    (0x3e, 0x02, "Timeout On Logical Unit"),
    (0x3e, 0x03, "Logical Unit Failed Self-Test"),
    (0x3e, 0x04, "Logical Unit Unable To Update Self-Test Log"),
    (0x3f, 0x00, "Target Operating Conditions Have Changed"),
    (0x3f, 0x01, "Microcode Has Changed"),
    (0x3f, 0x02, "Changed Operating Definition"),
    (0x3f, 0x03, "INQUIRY Data Has Changed"),
    (0x3f, 0x04, "component Device Attached"),
    (0x3f, 0x05, "Device Identifier Changed"),
    (0x3f, 0x06, "Redundancy Group Created Or Modified"),
    (0x3f, 0x07, "Redundancy Group Deleted"),
    (0x3f, 0x08, "Spare Created Or Modified"),
    (0x3f, 0x09, "Spare Deleted"),
    (0x3f, 0x0a, "Volume Set Created Or Modified"),
    (0x3f, 0x0b, "Volume Set Deleted"),
    (0x3f, 0x0c, "Volume Set Deassigned"),
    (0x3f, 0x0d, "Volume Set Reassigned"),
    (0x3f, 0x0e, "Reported LUNs Data Has Changed"),
    (0x3f, 0x0f, "Echo Buffer Overwritten"),
    (0x3f, 0x10, "Medium Loadable"),
    (0x3f, 0x11, "Medium Auxiliary Memory Accessible"),
    (0x3f, 0x12, "iSCSI IP Address Added"),
    (0x3f, 0x13, "iSCSI IP Address Removed"),
    (0x3f, 0x14, "iSCSI IP Address Changed"),
    (0x40, 0x00, "RAM FAILURE (Should Use 40 NN)"),
    (0x41, 0x00, "Data Path FAILURE (Should Use 40 NN)"),
    (
        0x42,
        0x00,
        "Power-On or Self-Test FAILURE (Should Use 40 NN)",
    ),
    (0x43, 0x00, "Message Error"),
    (0x44, 0x00, "Internal Target Failure"),
    (0x44, 0x71, "ATA Device Failed Set Features"),
    (0x45, 0x00, "Select Or Reselect Failure"),
    (0x46, 0x00, "Unsuccessful Soft Reset"),
    (0x47, 0x00, "SCSI Parity Error"),
    (0x47, 0x01, "Data Phase CRC Error Detected"),
    (
        0x47,
        0x02,
        "SCSI Parity Error Detected During ST Data Phase",
    ),
    (0x47, 0x03, "Information Unit iuCRC Error Detected"),
    (
        0x47,
        0x04,
        "Asynchronous Information Protection Error Detected",
    ),
    (0x47, 0x05, "Protocol Service CRC Error"),
    (0x47, 0x06, "PHY Test Function In Progress"),
    (0x47, 0x7f, "Some Commands Cleared By iSCSI Protocol Event"),
    (0x48, 0x00, "Initiator Detected Error Message Received"),
    (0x49, 0x00, "Invalid Message Error"),
    (0x4a, 0x00, "Command Phase Error"),
    (0x4b, 0x00, "Data Phase Error"),
    (0x4b, 0x01, "Invalid Target Port Transfer Tag Received"),
    (0x4b, 0x02, "Too Much Write Data"),
    (0x4b, 0x03, "ACK/NAK Timeout"),
    (0x4b, 0x04, "NAK Received"),
    (0x4b, 0x05, "Data Offset Error"),
    (0x4b, 0x06, "Initiator Response Timeout"),
    (0x4b, 0x07, "Connection Lost"),
    (0x4c, 0x00, "Logical Unit Failed Self-Configuration"),
    (0x4e, 0x00, "Overlapped Commands Attempted"),
    (0x50, 0x00, "Write Append Error"),
    (0x50, 0x01, "Write Append Position Error"),
    (0x50, 0x02, "Position Error Related To Timing"),
    (0x51, 0x00, "Erase Failure"),
    (
        0x51,
        0x01,
        "Erase Failure - Incomplete Erase Operation Detected",
    ),
    (0x52, 0x00, "Cartridge Fault"),
    (0x53, 0x00, "Media Load or Eject Failed"),
    (0x53, 0x01, "Unload Tape Failure"),
    (0x53, 0x02, "Medium Removal Prevented"),
    (
        0x53,
        0x03,
        "Medium Removal Prevented By Data Transfer Element",
    ),
    (0x53, 0x04, "Medium Thread Or Unthread Failure"),
    (0x53, 0x05, "Volume Identifier Invalid"),
    (0x53, 0x06, "Volume Identifier Missing"),
    (0x53, 0x07, "Duplicate Volume Identifier"),
    (0x53, 0x08, "Element Status Unknown"),
    (0x54, 0x00, "SCSI To Host System Interface Failure"),
    (0x55, 0x00, "System Resource Failure"),
    (0x55, 0x01, "System Buffer Full"),
    (0x55, 0x02, "Insufficient Reservation Resources"),
    (0x55, 0x03, "Insufficient Resources"),
    (0x55, 0x04, "Insufficient Registration Resources"),
    (0x55, 0x05, "Insufficient Access Control Resources"),
    (0x55, 0x06, "Auxiliary Memory Out Of Space"),
    (0x55, 0x07, "Quota Error"),
    (
        0x55,
        0x08,
        "Maximum Number Of Supplemental Decryption Keys Exceeded",
    ),
    (0x55, 0x09, "Medium Auxiliary Memory Not Accessible"),
    (0x55, 0x0a, "Data Currently Unavailable"),
    (0x55, 0x0b, "Insufficient Power For Operation"),
    (0x57, 0x00, "Unable To Recover Table-Of-Contents"),
    (0x58, 0x00, "Generation Does Not Exist"),
    (0x59, 0x00, "Updated Block Read"),
    (0x5a, 0x00, "Operator Request or State Change Input"),
    (0x5a, 0x01, "Operator Medium Removal Requested"),
    (0x5a, 0x02, "Operator Selected Write Protect"),
    (0x5a, 0x03, "Operator Selected Write Permit"),
    (0x5b, 0x00, "Log Exception"),
    (0x5b, 0x01, "Threshold Condition Met"),
    (0x5b, 0x02, "Log Counter At Maximum"),
    (0x5b, 0x03, "Log List Codes Exhausted"),
    (0x5c, 0x00, "RPL Status Change"),
    (0x5c, 0x01, "Spindles Synchronized"),
    (0x5c, 0x02, "Spindles Not Synchronized"),
    (0x5d, 0x00, "Failure Prediction Threshold Exceeded"),
    (0x5d, 0x01, "Media Failure Prediction Threshold Exceeded"),
    (
        0x5d,
        0x02,
        "Logical Unit Failure Prediction Threshold Exceeded",
    ),
    (
        0x5d,
        0x03,
        "Spare Area Exhaustion Prediction Threshold Exceeded",
    ),
    (
        0x5d,
        0x10,
        "Hardware Impending Failure General Hard Drive Failure",
    ),
    (
        0x5d,
        0x11,
        "Hardware Impending Failure Drive Error Rate Too High",
    ),
    (
        0x5d,
        0x12,
        "Hardware Impending Failure Data Error Rate Too High",
    ),
    (
        0x5d,
        0x13,
        "Hardware Impending Failure Seek Error Rate Too High",
    ),
    (
        0x5d,
        0x14,
        "Hardware Impending Failure Too Many Block Reassigns",
    ),
    (
        0x5d,
        0x15,
        "Hardware Impending Failure Access Times Too High",
    ),
    (
        0x5d,
        0x16,
        "Hardware Impending Failure Start Unit Times Too High",
    ),
    (0x5d, 0x17, "Hardware Impending Failure Channel Parametrics"),
    (0x5d, 0x18, "Hardware Impending Failure Controller Detected"),
    (
        0x5d,
        0x19,
        "Hardware Impending Failure Throughput Performance",
    ),
    (
        0x5d,
        0x1a,
        "Hardware Impending Failure Seek Time Performance",
    ),
    (0x5d, 0x1b, "Hardware Impending Failure Spin-Up Retry Count"),
    (
        0x5d,
        0x1c,
        "Hardware Impending Failure Drive Calibration Retry Count",
    ),
    (
        0x5d,
        0x20,
        "Controller Impending Failure General Hard Drive Failure",
    ),
    (
        0x5d,
        0x21,
        "Controller Impending Failure Drive Error Rate Too High",
    ),
    (
        0x5d,
        0x22,
        "Controller Impending Failure Data Error Rate Too High",
    ),
    (
        0x5d,
        0x23,
        "Controller Impending Failure Seek Error Rate Too High",
    ),
    (
        0x5d,
        0x24,
        "Controller Impending Failure Too Many Block Reassigns",
    ),
    (
        0x5d,
        0x25,
        "Controller Impending Failure Access Times Too High",
    ),
    (
        0x5d,
        0x26,
        "Controller Impending Failure Start Unit Times Too High",
    ),
    (
        0x5d,
        0x27,
        "Controller Impending Failure Channel Parametrics",
    ),
    (
        0x5d,
        0x28,
        "Controller Impending Failure Controller Detected",
    ),
    (
        0x5d,
        0x29,
        "Controller Impending Failure Throughput Performance",
    ),
    (
        0x5d,
        0x2a,
        "Controller Impending Failure Seek Time Performance",
    ),
    (
        0x5d,
        0x2b,
        "Controller Impending Failure Spin-Up Retry Count",
    ),
    (
        0x5d,
        0x2c,
        "Controller Impending Failure Drive Calibration Retry Count",
    ),
    (
        0x5d,
        0x30,
        "Data Channel Impending Failure General Hard Drive Failure",
    ),
    (
        0x5d,
        0x31,
        "Data Channel Impending Failure Drive Error Rate Too High",
    ),
    (
        0x5d,
        0x32,
        "Data Channel Impending Failure Data Error Rate Too High",
    ),
    (
        0x5d,
        0x33,
        "Data Channel Impending Failure Seek Error Rate Too High",
    ),
    (
        0x5d,
        0x34,
        "Data Channel Impending Failure Too Many Block Reassigns",
    ),
    (
        0x5d,
        0x35,
        "Data Channel Impending Failure Access Times Too High",
    ),
    (
        0x5d,
        0x36,
        "Data Channel Impending Failure Start Unit Times Too High",
    ),
    (
        0x5d,
        0x37,
        "Data Channel Impending Failure Channel Parametrics",
    ),
    (
        0x5d,
        0x38,
        "Data Channel Impending Failure Controller Detected",
    ),
    (
        0x5d,
        0x39,
        "Data Channel Impending Failure Throughput Performance",
    ),
    (
        0x5d,
        0x3a,
        "Data Channel Impending Failure Seek Time Performance",
    ),
    (
        0x5d,
        0x3b,
        "Data Channel Impending Failure Spin-Up Retry Count",
    ),
    (
        0x5d,
        0x3c,
        "Data Channel Impending Failure Drive Calibration Retry Count",
    ),
    (
        0x5d,
        0x40,
        "Servo Impending Failure General Hard Drive Failure",
    ),
    (
        0x5d,
        0x41,
        "Servo Impending Failure Drive Error Rate Too High",
    ),
    (
        0x5d,
        0x42,
        "Servo Impending Failure Data Error Rate Too High",
    ),
    (
        0x5d,
        0x43,
        "Servo Impending Failure Seek Error Rate Too High",
    ),
    (
        0x5d,
        0x44,
        "Servo Impending Failure Too Many Block Reassigns",
    ),
    (0x5d, 0x45, "Servo Impending Failure Access Times Too High"),
    (
        0x5d,
        0x46,
        "Servo Impending Failure Start Unit Times Too High",
    ),
    (0x5d, 0x47, "Servo Impending Failure Channel Parametrics"),
    (0x5d, 0x48, "Servo Impending Failure Controller Detected"),
    (0x5d, 0x49, "Servo Impending Failure Throughput Performance"),
    (0x5d, 0x4a, "Servo Impending Failure Seek Time Performance"),
    (0x5d, 0x4b, "Servo Impending Failure Spin-Up Retry Count"),
    (
        0x5d,
        0x4c,
        "Servo Impending Failure Drive Calibration Retry Count",
    ),
    (
        0x5d,
        0x50,
        "Spindle Impending Failure General Hard Drive Failure",
    ),
    (
        0x5d,
        0x51,
        "Spindle Impending Failure Drive Error Rate Too High",
    ),
    (
        0x5d,
        0x52,
        "Spindle Impending Failure Data Error Rate Too High",
    ),
    (
        0x5d,
        0x53,
        "Spindle Impending Failure Seek Error Rate Too High",
    ),
    (
        0x5d,
        0x54,
        "Spindle Impending Failure Too Many Block Reassigns",
    ),
    (
        0x5d,
        0x55,
        "Spindle Impending Failure Access Times Too High",
    ),
    (
        0x5d,
        0x56,
        "Spindle Impending Failure Start Unit Times Too High",
    ),
    (0x5d, 0x57, "Spindle Impending Failure Channel Parametrics"),
    (0x5d, 0x58, "Spindle Impending Failure Controller Detected"),
    (
        0x5d,
        0x59,
        "Spindle Impending Failure Throughput Performance",
    ),
    (
        0x5d,
        0x5a,
        "Spindle Impending Failure Seek Time Performance",
    ),
    (0x5d, 0x5b, "Spindle Impending Failure Spin-Up Retry Count"),
    (
        0x5d,
        0x5c,
        "Spindle Impending Failure Drive Calibration Retry Count",
    ),
    (
        0x5d,
        0x60,
        "Firmware Impending Failure General Hard Drive Failure",
    ),
    (
        0x5d,
        0x61,
        "Firmware Impending Failure Drive Error Rate Too High",
    ),
    (
        0x5d,
        0x62,
        "Firmware Impending Failure Data Error Rate Too High",
    ),
    (
        0x5d,
        0x63,
        "Firmware Impending Failure Seek Error Rate Too High",
    ),
    (
        0x5d,
        0x64,
        "Firmware Impending Failure Too Many Block Reassigns",
    ),
    (
        0x5d,
        0x65,
        "Firmware Impending Failure Access Times Too High",
    ),
    (
        0x5d,
        0x66,
        "Firmware Impending Failure Start Unit Times Too High",
    ),
    (0x5d, 0x67, "Firmware Impending Failure Channel Parametrics"),
    (0x5d, 0x68, "Firmware Impending Failure Controller Detected"),
    (
        0x5d,
        0x69,
        "Firmware Impending Failure Throughput Performance",
    ),
    (
        0x5d,
        0x6a,
        "Firmware Impending Failure Seek Time Performance",
    ),
    (0x5d, 0x6b, "Firmware Impending Failure Spin-Up Retry Count"),
    (
        0x5d,
        0x6c,
        "Firmware Impending Failure Drive Calibration Retry Count",
    ),
    (0x5d, 0xff, "Failure Prediction Threshold Exceeded (false)"),
    (0x5e, 0x00, "Low Power Condition On"),
    (0x5e, 0x01, "Idle Condition Activated By Timer"),
    (0x5e, 0x02, "Standby Condition Activated By Timer"),
    (0x5e, 0x03, "Idle Condition Activated By Command"),
    (0x5e, 0x04, "Standby Condition Activated By Command"),
    (0x5e, 0x05, "IDLE_B Condition Activated By Timer"),
    (0x5e, 0x06, "IDLE_B Condition Activated By Command"),
    (0x5e, 0x07, "IDLE_C Condition Activated By Timer"),
    (0x5e, 0x08, "IDLE_C Condition Activated By Command"),
    (0x5e, 0x09, "STANDBY_Y Condition Activated By Timer"),
    (0x5e, 0x0a, "STANDBY_Y Condition Activated By Command"),
    (0x5e, 0x41, "Power State Change To Active"),
    (0x5e, 0x42, "Power State Change To Idle"),
    (0x5e, 0x43, "Power State Change To Standby"),
    (0x5e, 0x45, "Power State Change To Sleep"),
    (0x5e, 0x47, "Power State Change To Device Control"),
    (0x60, 0x00, "Lamp Failure"),
    (0x61, 0x00, "Video Acquisition Error"),
    (0x61, 0x01, "Unable To Acquire Video"),
    (0x61, 0x02, "Out Of Focus"),
    (0x62, 0x00, "Scan Head Positioning Error"),
    (0x63, 0x00, "End Of User Area Encountered On This Track"),
    (0x63, 0x01, "Packet Does Not Fit In Available Space"),
    (0x64, 0x00, "Illegal Mode For This Track"),
    (0x64, 0x01, "Invalid Packet Size"),
    (0x65, 0x00, "Voltage Fault"),
    (0x66, 0x00, "Automatic Document Feeder Cover Up"),
    (0x66, 0x01, "Automatic Document Feeder Lift Up"),
    (0x66, 0x02, "Document Jam In Automatic Document Feeder"),
    (
        0x66,
        0x03,
        "Document Miss Feed Automatic In Document Feeder",
    ),
    (0x67, 0x00, "Configuration Failure"),
    (
        0x67,
        0x01,
        "Configuration Of Incapable Logical Units Failed",
    ),
    (0x67, 0x02, "Add Logical Unit Failed"),
    (0x67, 0x03, "Modification Of Logical Unit Failed"),
    (0x67, 0x04, "Exchange Of Logical Unit Failed"),
    (0x67, 0x05, "Remove Of Logical Unit Failed"),
    (0x67, 0x06, "Attachment Of Logical Unit Failed"),
    (0x67, 0x07, "Creation Of Logical Unit Failed"),
    (0x67, 0x08, "Assign Failure Occurred"),
    (0x67, 0x09, "Multiply Assigned Logical Unit"),
    (0x67, 0x0a, "Set Target Port Groups Command Failed"),
    (0x67, 0x0b, "ATA Device Feature Not Enabled"),
    (0x68, 0x00, "Logical Unit Not Configured"),
    (0x69, 0x00, "Data Loss On Logical Unit"),
    (0x69, 0x01, "Multiple Logical Unit Failures"),
    (0x69, 0x02, "Parity/Data Mismatch"),
    (0x6a, 0x00, "Informational, Refer To Log"),
    (0x6b, 0x00, "State Change Has Occurred"),
    (0x6b, 0x01, "Redundancy Level Got Better"),
    (0x6b, 0x02, "Redundancy Level Got Worse"),
    (0x6c, 0x00, "Rebuild Failure Occurred"),
    (0x6d, 0x00, "Recalculate Failure Occurred"),
    (0x6e, 0x00, "Command To Logical Unit Failed"),
    (
        0x6f,
        0x00,
        "Copy Protection Key Exchange Failure - Authentication Failure",
    ),
    (
        0x6f,
        0x01,
        "Copy Protection Key Exchange Failure - Key Not Present",
    ),
    (
        0x6f,
        0x02,
        "Copy Protection Key Exchange Failure - Key Not Established",
    ),
    (
        0x6f,
        0x03,
        "Read Of Scrambled Sector Without Authentication",
    ),
    (
        0x6f,
        0x04,
        "Media Region Code Is Mismatched To Logical Unit Region",
    ),
    (
        0x6f,
        0x05,
        "Drive Region Must Be Permanent/Region Reset Count Error",
    ),
    (0x71, 0x00, "Decompression Exception Long Algorithm ID"),
    (0x72, 0x00, "Session Fixation Error"),
    (0x72, 0x01, "Session Fixation Error Writing Lead-In"),
    (0x72, 0x02, "Session Fixation Error Writing Lead-Out"),
    (
        0x72,
        0x03,
        "Session Fixation Error - Incomplete Track In Session",
    ),
    (0x72, 0x04, "Empty Or Partially Written Reserved Track"),
    (0x72, 0x05, "No More Track Reservations Allowed"),
    (0x72, 0x06, "RMZ Extension Is Not Allowed"),
    (0x72, 0x07, "No More Test Zone Extensions Are Allowed"),
    (0x73, 0x00, "CD Control Error"),
    (0x73, 0x01, "Power Calibration Area Almost Full"),
    (0x73, 0x02, "Power Calibration Area Is Full"),
    (0x73, 0x03, "Power Calibration Area Error"),
    (0x73, 0x04, "Program Memory Area Update Failure"),
    (0x73, 0x05, "Program Memory Area Is Full"),
    (0x73, 0x06, "RMA/PMA Is Almost Full"),
    (0x73, 0x10, "Current Power Calibration Area Almost Full"),
    (0x73, 0x11, "Current Power Calibration Area Is Full"),
    (0x73, 0x17, "RDZ Is Full"),
    (0x74, 0x00, "Security Error"),
    (0x74, 0x01, "Unable To Decrypt Data"),
    (0x74, 0x02, "Unencrypted Data Encountered While Decrypting"),
    (0x74, 0x03, "Incorrect Data Encryption Key"),
    (0x74, 0x04, "Cryptographic Integrity Validation Failed"),
    (0x74, 0x05, "Error Decrypting Data"),
    (0x74, 0x06, "Unknown Signature Verification Key"),
    (0x74, 0x07, "Encryption Parameters Not Useable"),
    (0x74, 0x08, "Digital Signature Validation Failure"),
    (0x74, 0x09, "Encryption Mode Mismatch On Read"),
    (0x74, 0x0a, "Encrypted Block Not Raw Read Enabled"),
    (0x74, 0x0b, "Incorrect Encryption Parameters"),
    (0x74, 0x0c, "Unable To Decrypt Parameter List"),
    (0x74, 0x0d, "Encryption Algorithm Disabled"),
    (0x74, 0x10, "SA Creation Parameter Value Invalid"),
    (0x74, 0x11, "SA Creation Parameter Value Rejected"),
    (0x74, 0x12, "Invalid SA Usage"),
    (0x74, 0x21, "Data Encryption Configuration Prevented"),
    (0x74, 0x30, "SA Creation Parameter Not Supported"),
    (0x74, 0x40, "Authentication Failed"),
    (
        0x74,
        0x61,
        "External Data Encryption Key Manager Access Error",
    ),
    (0x74, 0x62, "External Data Encryption Key Manager Error"),
    (0x74, 0x63, "External Data Encryption Key Not Found"),
    (
        0x74,
        0x64,
        "External Data Encryption Request Not Authorized",
    ),
    (0x74, 0x6e, "External Data Encryption Control Timeout"),
    (0x74, 0x6f, "External Data Encryption Control Error"),
    (0x74, 0x71, "Logical Unit Access Not Authorized"),
    (0x74, 0x79, "Security Conflict In Translated Device"),
];

/// `struct scsi_plug`: a deferred probe or detach (`scsi_req_probe`, `scsi_req_detach`), a
/// `scsi_plug_pool` item until its task runs.
struct ScsiPlug {
    /// `task`: runs `scsi_plug_probe` or `scsi_plug_detach` on `systq`.
    task: Task,
    /// `sb`: the bus.
    sb: &'static ScsibusSoftc,
    /// `target`.
    target: i32,
    /// `lun`.
    lun: i32,
    /// `how`: the detach flags.
    how: i32,
}

/// `struct scsi_io_mover`: moves an opening from a run queue to a process waiting for one
/// (the synchronous API for allocating an I/O).
struct ScsiIoMover {
    /// `mtx`.
    mtx: Mutex,
    /// `io`: the opening, or `None` when the pool or the link shut down.
    io: Cell<Option<ScsiIo>>,
    /// `done`: the handler ran.
    done: Cell<bool>,
}

impl ScsiIoMover {
    /// `SCSI_IO_MOVER_INITIALIZER`.
    const fn new() -> Self {
        Self {
            mtx: Mutex::new(IPL_BIO),
            io: Cell::new(None),
            done: Cell::new(false),
        }
    }
}

/// `%#x` of the C's printf: `0` for zero, else `0x` and the hexadecimal digits.
struct AltHex(u32);

impl fmt::Display for AltHex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == 0 {
            f.write_str("0")
        } else {
            write!(f, "{:#x}", self.0)
        }
    }
}

/// `scsi_xfer_pool`: the transfers.
pub static SCSI_XFER_POOL: Pool = Pool::new();
/// `scsi_plug_pool`: the deferred probes and detaches.
pub static SCSI_PLUG_POOL: Pool = Pool::new();
/// `scsi_init`'s `static int scsi_init_done`.
static SCSI_INIT_DONE: AtomicBool = AtomicBool::new(false);

/// `scsi_init`: called when a scsibus is attached, to initialise the global data (once).
pub fn scsi_init() {
    if SCSI_INIT_DONE.swap(true, Ordering::Relaxed) {
        return;
    }

    // SCSI_DELAY (historical: older buses may need a moment to stabilize) is not configured.

    // Initialize the scsi_xfer pool.
    pool_init(
        &SCSI_XFER_POOL,
        size_of::<ScsiXfer>(),
        0,
        IPL_BIO,
        0,
        "scxspl",
        None,
    );
    pool_setlowat(&SCSI_XFER_POOL, 8);
    // The C ignores whether the priming worked; the low watermark refills the pool later.
    let _ = pool_prime(&SCSI_XFER_POOL, 8);
    pool_init(
        &SCSI_PLUG_POOL,
        size_of::<ScsiPlug>(),
        0,
        IPL_BIO,
        0,
        "scsiplug",
        None,
    );
}

/// Takes a `scsi_plug_pool` item for `func` and queues its task on `systq`.
fn scsi_plug_add(
    sb: &'static ScsibusSoftc,
    target: i32,
    lun: i32,
    how: i32,
    func: fn(*mut c_void),
) -> Result<(), Errno> {
    let Some(mem) = pool_get(&SCSI_PLUG_POOL, PR_NOWAIT) else {
        return Err(ENOMEM);
    };
    let p = mem.cast::<ScsiPlug>();
    // SAFETY: a fresh pool item of `size_of::<ScsiPlug>()` bytes, suitably aligned, written
    // once before anything else sees it.
    unsafe {
        p.as_ptr().write(ScsiPlug {
            task: Task::new(func, p.as_ptr().cast()),
            sb,
            target,
            lun,
            how,
        });
    }
    // SAFETY: the item stays allocated until the task's function gives it back, after the
    // task queue has copied the task out of it (`taskq_next_work`).
    let plug: &'static ScsiPlug = unsafe { p.as_ref() };
    task_add(SYSTQ, &plug.task);
    Ok(())
}

/// `scsi_req_probe`: probes `target`/`lun` of `sb` (-1: all) from `systq`.
pub fn scsi_req_probe(sb: &'static ScsibusSoftc, target: i32, lun: i32) -> Result<(), Errno> {
    scsi_plug_add(sb, target, lun, 0, scsi_plug_probe)
}

/// `scsi_req_detach`: detaches `target`/`lun` of `sb` (-1: all) from `systq`.
pub fn scsi_req_detach(
    sb: &'static ScsibusSoftc,
    target: i32,
    lun: i32,
    how: i32,
) -> Result<(), Errno> {
    scsi_plug_add(sb, target, lun, how, scsi_plug_detach)
}

/// Reads a plug and gives it back to its pool.
///
/// # Safety
///
/// `xp` is the `ScsiPlug` item whose task is running; nothing uses it afterwards.
unsafe fn scsi_plug_take(xp: *mut c_void) -> (&'static ScsibusSoftc, i32, i32, i32) {
    // SAFETY: the caller's contract: a live plug.
    let p = unsafe { &*xp.cast::<ScsiPlug>() };
    let args = (p.sb, p.target, p.lun, p.how);
    pool_put(&SCSI_PLUG_POOL, NonNull::from(p).cast());
    args
}

/// `scsi_plug_probe`: the task of `scsi_req_probe`.
fn scsi_plug_probe(xp: *mut c_void) {
    // SAFETY: the task's argument is its own plug (`scsi_plug_add`), and it runs once.
    let (sb, target, lun, _) = unsafe { scsi_plug_take(xp) };

    // The C ignores the result; the probe reports its own failures.
    let _ = scsi_probe(sb, target, lun);
}

/// `scsi_plug_detach`: the task of `scsi_req_detach`.
fn scsi_plug_detach(xp: *mut c_void) {
    // SAFETY: as in `scsi_plug_probe`.
    let (sb, target, lun, how) = unsafe { scsi_plug_take(xp) };

    // The C ignores the result.
    let _ = scsi_detach(sb, target, lun, how);
}

/// `scsi_pending_start`: enters a run queue loop; `false` when another one is running (it
/// will go round once more for us).
pub fn scsi_pending_start(mtx: &Mutex, running: &Cell<u32>) -> bool {
    let mut rv = true;

    mtx_enter(mtx);
    running.set(running.get() + 1);
    if running.get() > 1 {
        rv = false;
    }
    mtx_leave(mtx);

    rv
}

/// `scsi_pending_finish`: leaves a run queue loop; `false` when someone asked for another
/// round meanwhile.
pub fn scsi_pending_finish(mtx: &Mutex, running: &Cell<u32>) -> bool {
    let mut rv = true;

    mtx_enter(mtx);
    running.set(running.get() - 1);
    if running.get() > 0 {
        running.set(1);
        rv = false;
    }
    mtx_leave(mtx);

    rv
}

/// `scsi_iopool_init`: sets up `iopl` to hand out the openings of `io_get`/`io_put`.
///
/// # Safety
///
/// `io_get` and `io_put` accept `iocookie` (the contract of [`ScsiIoGetFn`] and
/// [`ScsiIoPutFn`]), and `iocookie` stays valid while the pool is used.
pub unsafe fn scsi_iopool_init(
    iopl: &ScsiIopool,
    iocookie: *mut c_void,
    io_get: ScsiIoGetFn,
    io_put: ScsiIoPutFn,
) {
    iopl.iocookie.set(iocookie);
    iopl.io_get.set(Some(io_get));
    iopl.io_put.set(Some(io_put));

    iopl.queue.init();
    iopl.running.set(0);
    mtx_init(&iopl.mtx, IPL_BIO);
}

/// `scsi_iopool_get`: one opening from the backend, if one is free.
fn scsi_iopool_get(iopl: &ScsiIopool) -> Option<ScsiIo> {
    let Some(io_get) = iopl.io_get.get() else {
        panic(format_args!(
            "scsi_iopool_get: iopool {:p} not initialised",
            iopl
        ));
    };
    kernel_lock();
    // SAFETY: `scsi_iopool_init` paired `io_get` with `iocookie`.
    let io = unsafe { io_get(iopl.iocookie.get()) };
    kernel_unlock();

    io
}

/// `scsi_iopool_put`: gives an opening back to the backend.
fn scsi_iopool_put(iopl: &ScsiIopool, io: ScsiIo) {
    let Some(io_put) = iopl.io_put.get() else {
        panic(format_args!(
            "scsi_iopool_put: iopool {:p} not initialised",
            iopl
        ));
    };
    kernel_lock();
    // SAFETY: `scsi_iopool_init` paired `io_put` with `iocookie`; `io` came from the paired
    // `io_get` (every opening of this pool does).
    unsafe { io_put(iopl.iocookie.get(), io) };
    kernel_unlock();
}

/// Calls an I/O handler with an opening, or `None`.
fn scsi_ioh_call(ioh: &ScsiIohandler, io: Option<ScsiIo>) {
    let Some(handler) = ioh.handler.get() else {
        panic(format_args!("scsi_iohandler {:p} has no handler", ioh));
    };
    // SAFETY: `scsi_ioh_set` paired `handler` with `cookie`.
    unsafe { handler(ioh.cookie.get(), io) };
}

/// Whether `ioh`'s handler is `f`.
fn scsi_ioh_is(ioh: &ScsiIohandler, f: ScsiIohFn) -> bool {
    ioh.handler.get().is_some_and(|h| ptr::fn_addr_eq(h, f))
}

/// `scsi_iopool_destroy`: empties the pool's run queue; the processes sleeping in
/// `scsi_io_get` get no opening.
pub fn scsi_iopool_destroy(iopl: &ScsiIopool) {
    let sleepers = ScsiRunq::new();

    mtx_enter(&iopl.mtx);
    while let Some(ioh) = iopl.queue.first() {
        // SAFETY: `ioh` is on this queue; the pool's mutex is held.
        unsafe { iopl.queue.remove(ioh) };
        ioh.q_state.set(RUNQ_IDLE);

        if scsi_ioh_is(ioh, scsi_io_get_done) {
            // SAFETY: just unlinked; a `scsi_io_get` waiter stays in place until its handler
            // has run, and `sleepers` does not move while it holds elements.
            unsafe { sleepers.insert_tail(ioh) };
        } else {
            #[cfg(feature = "diagnostic")]
            panic(format_args!("scsi_iopool_destroy: scsi_iohandler on pool"));
        }
    }
    mtx_leave(&iopl.mtx);

    while let Some(ioh) = sleepers.first() {
        // SAFETY: `ioh` is on `sleepers`.
        unsafe { sleepers.remove(ioh) };
        scsi_ioh_call(ioh, None);
    }
}

/// `scsi_default_get`: the default io allocator for links without an adapter pool: every
/// opening is [`SCSI_IOPOOL_POISON`] (the link's `openings` do the metering).
pub fn scsi_default_get(iocookie: *mut c_void) -> Option<ScsiIo> {
    let _ = iocookie;
    Some(SCSI_IOPOOL_POISON)
}

/// `scsi_default_put`: takes a [`scsi_default_get`] opening back.
pub fn scsi_default_put(iocookie: *mut c_void, io: ScsiIo) {
    let _ = (iocookie, io);
    #[cfg(feature = "diagnostic")]
    if io != SCSI_IOPOOL_POISON {
        panic(format_args!("unexpected opening returned"));
    }
}

/*
 * public interface to the ioh api.
 */

/// `scsi_ioh_set`: sets up `ioh` to call `handler(cookie, io)` with an opening of `iopl`.
///
/// # Safety
///
/// `handler` accepts `cookie` (the contract of [`ScsiIohFn`]), and `cookie` stays valid
/// while `ioh` is queued or its handler may run.
pub unsafe fn scsi_ioh_set(
    ioh: &ScsiIohandler,
    iopl: &'static ScsiIopool,
    handler: ScsiIohFn,
    cookie: *mut c_void,
) {
    ioh.q_state.set(RUNQ_IDLE);
    ioh.pool.set(Some(iopl));
    ioh.handler.set(Some(handler));
    ioh.cookie.set(cookie);
    ioh.is_xsh.set(false);
}

/// The pool of an I/O handler, which `scsi_ioh_set` always sets.
fn scsi_ioh_pool(ioh: &ScsiIohandler) -> &'static ScsiIopool {
    match ioh.pool.get() {
        Some(pool) => pool,
        None => panic(format_args!("scsi_iohandler {:p} has no pool", ioh)),
    }
}

/// `scsi_ioh_add`: queues `ioh` on its pool and gets some I/O going; `true` when it was
/// not queued yet.
pub fn scsi_ioh_add(ioh: &'static ScsiIohandler) -> bool {
    // SAFETY: a `'static` handler stays in place.
    unsafe { scsi_ioh_add_unchecked(ioh) }
}

/// [`scsi_ioh_add`] for a handler that is not `'static` (`scsi_io_get`'s, `scsi_xs_get`'s).
///
/// # Safety
///
/// `ioh` stays in place until it has left the pool's queue (its handler has been called, or
/// `scsi_ioh_del` has taken it off).
unsafe fn scsi_ioh_add_unchecked(ioh: &ScsiIohandler) -> bool {
    let iopl = scsi_ioh_pool(ioh);
    let mut rv = false;

    mtx_enter(&iopl.mtx);
    match ioh.q_state.get() {
        RUNQ_IDLE => {
            // SAFETY: an idle handler is on no queue; the caller keeps it in place while it
            // is on this one; the pool's mutex is held.
            unsafe { iopl.queue.insert_tail(ioh) };
            ioh.q_state.set(RUNQ_POOLQ);
            rv = true;
        }
        #[cfg(feature = "diagnostic")]
        RUNQ_POOLQ => {}
        #[cfg(feature = "diagnostic")]
        state => panic(format_args!("scsi_ioh_add: unexpected state {state}")),
        #[cfg(not(feature = "diagnostic"))]
        _ => {}
    }
    mtx_leave(&iopl.mtx);

    // lets get some io up in the air
    scsi_iopool_run(iopl);

    rv
}

/// `scsi_ioh_del`: takes `ioh` off its pool's queue; `true` when it was on it.
pub fn scsi_ioh_del(ioh: &ScsiIohandler) -> bool {
    let iopl = scsi_ioh_pool(ioh);
    let mut rv = false;

    mtx_enter(&iopl.mtx);
    match ioh.q_state.get() {
        RUNQ_POOLQ => {
            // SAFETY: a handler in state `RUNQ_POOLQ` is on its pool's queue; the mutex is
            // held.
            unsafe { iopl.queue.remove(ioh) };
            ioh.q_state.set(RUNQ_IDLE);
            rv = true;
        }
        #[cfg(feature = "diagnostic")]
        RUNQ_IDLE => {}
        #[cfg(feature = "diagnostic")]
        state => panic(format_args!("scsi_ioh_del: unexpected state {state}")),
        #[cfg(not(feature = "diagnostic"))]
        _ => {}
    }
    mtx_leave(&iopl.mtx);

    rv
}

/*
 * internal iopool runqueue handling.
 */

/// `scsi_ioh_deq`: takes the first handler off the pool's queue.
fn scsi_ioh_deq(iopl: &ScsiIopool) -> Option<&ScsiIohandler> {
    mtx_enter(&iopl.mtx);
    let ioh = iopl.queue.first();
    if let Some(ioh) = ioh {
        // SAFETY: `ioh` is on this queue; the mutex is held. Whoever queued it keeps it in
        // place until its handler has run, which is what the caller does next.
        unsafe { iopl.queue.remove(ioh) };
        ioh.q_state.set(RUNQ_IDLE);
    }
    mtx_leave(&iopl.mtx);

    ioh
}

/// `scsi_ioh_pending`: whether handlers wait on the pool.
fn scsi_ioh_pending(iopl: &ScsiIopool) -> bool {
    mtx_enter(&iopl.mtx);
    let rv = !iopl.queue.is_empty();
    mtx_leave(&iopl.mtx);

    rv
}

/// `scsi_iopool_run`: hands free openings to the waiting handlers, in queue order.
pub fn scsi_iopool_run(iopl: &ScsiIopool) {
    if !scsi_pending_start(&iopl.mtx, &iopl.running) {
        return;
    }
    loop {
        while scsi_ioh_pending(iopl) {
            let Some(io) = scsi_iopool_get(iopl) else {
                break;
            };

            let Some(ioh) = scsi_ioh_deq(iopl) else {
                scsi_iopool_put(iopl, io);
                break;
            };

            scsi_ioh_call(ioh, Some(io));
        }
        if scsi_pending_finish(&iopl.mtx, &iopl.running) {
            break;
        }
    }
}

/*
 * move an io from a runq to a proc that's waiting for an io.
 */

/// `scsi_move`: sleeps until the mover's handler has run.
fn scsi_move(m: &ScsiIoMover) {
    mtx_enter(&m.mtx);
    while !m.done.get() {
        // An uninterruptible sleep without a timeout only ends with the wakeup.
        let _ = msleep_nsec(ptr::from_ref(m), &m.mtx, PRIBIO, "scsiiomv", INFSLP);
    }
    mtx_leave(&m.mtx);
}

/// `scsi_move_done`: hands `io` to the process sleeping in `scsi_move`.
///
/// # Safety
///
/// `cookie` is a live `ScsiIoMover` whose owner sleeps in (or is about to enter)
/// `scsi_move`.
unsafe fn scsi_move_done(cookie: *mut c_void, io: Option<ScsiIo>) {
    // SAFETY: the caller's contract.
    let m = unsafe { &*cookie.cast::<ScsiIoMover>() };

    mtx_enter(&m.mtx);
    m.io.set(io);
    m.done.set(true);
    wakeup_one(ptr::from_ref(m));
    mtx_leave(&m.mtx);
}

/*
 * synchronous api for allocating an io.
 */

/// `scsi_io_get`: an opening of `iopl`, sleeping for one unless `SCSI_NOSLEEP` is in
/// `flags`; `None` when none is free (`SCSI_NOSLEEP`) or the pool was destroyed.
pub fn scsi_io_get(iopl: &'static ScsiIopool, flags: i32) -> Option<ScsiIo> {
    // try and sneak an io off the backend immediately
    if let Some(io) = scsi_iopool_get(iopl) {
        return Some(io);
    } else if flags & SCSI_NOSLEEP != 0 {
        return None;
    }

    // otherwise sleep until we get one
    let m = ScsiIoMover::new();
    let ioh = ScsiIohandler::new();
    // SAFETY: `scsi_io_get_done` takes an `ScsiIoMover`; `m` lives until `scsi_move`
    // returns, which is after the handler has run.
    unsafe {
        scsi_ioh_set(
            &ioh,
            iopl,
            scsi_io_get_done,
            ptr::from_ref(&m).cast_mut().cast(),
        )
    };
    // SAFETY: `ioh` stays on this stack until `scsi_move` returns, by which time its handler
    // has run and it is off the queue.
    unsafe { scsi_ioh_add_unchecked(&ioh) };
    scsi_move(&m);

    m.io.get()
}

/// `scsi_io_get_done`: the handler of `scsi_io_get`'s waiter.
///
/// # Safety
///
/// As [`scsi_move_done`].
unsafe fn scsi_io_get_done(cookie: *mut c_void, io: Option<ScsiIo>) {
    // SAFETY: forwarded.
    unsafe { scsi_move_done(cookie, io) }
}

/// `scsi_io_put`: gives an opening back and lets the waiting handlers have it.
pub fn scsi_io_put(iopl: &ScsiIopool, io: ScsiIo) {
    scsi_iopool_put(iopl, io);
    scsi_iopool_run(iopl);
}

/*
 * public interface to the xsh api.
 */

/// `scsi_xsh_set`: sets up `xsh` to call `handler` with a transfer for `link`.
pub fn scsi_xsh_set(
    xsh: &'static ScsiXshandler,
    link: &'static ScsiLink,
    handler: fn(xs: &'static ScsiXfer),
) {
    // SAFETY: `scsi_xsh_ioh` takes an `ScsiXshandler`, and `xsh` is `'static`.
    unsafe {
        scsi_ioh_set(
            &xsh.ioh,
            link.pool(),
            scsi_xsh_ioh,
            ptr::from_ref(xsh).cast_mut().cast(),
        );
    }
    xsh.ioh.is_xsh.set(true);

    xsh.link.set(Some(link));
    xsh.handler.set(Some(handler));
}

/// The link of a transfer handler, which `scsi_xsh_set` always sets.
fn scsi_xsh_link(xsh: &ScsiXshandler) -> &'static ScsiLink {
    match xsh.link.get() {
        Some(link) => link,
        None => panic(format_args!("scsi_xshandler {:p} has no link", xsh)),
    }
}

/// `scsi_xsh_add`: queues `xsh` on its link; its handler gets a transfer once the link and
/// the adapter have an opening. `false` when it was queued already or the link is dying.
pub fn scsi_xsh_add(xsh: &'static ScsiXshandler) -> bool {
    // SAFETY: a `'static` handler stays in place.
    unsafe { scsi_xsh_add_unchecked(xsh) }
}

/// [`scsi_xsh_add`] for a handler that is not `'static` (`scsi_xs_get`'s).
///
/// # Safety
///
/// `xsh` stays in place until it has left the link's and the pool's queues.
unsafe fn scsi_xsh_add_unchecked(xsh: &ScsiXshandler) -> bool {
    let link = scsi_xsh_link(xsh);
    let mut rv = false;

    if link.state.get() & SDEV_S_DYING != 0 {
        return false;
    }

    let pool = link.pool();
    mtx_enter(&pool.mtx);
    if xsh.ioh.q_state.get() == RUNQ_IDLE {
        // SAFETY: an idle handler is on no queue; the caller keeps it in place; the pool's
        // mutex protects the link's queue.
        unsafe { link.queue.insert_tail(&xsh.ioh) };
        xsh.ioh.q_state.set(RUNQ_LINKQ);
        rv = true;
    }
    mtx_leave(&pool.mtx);

    // lets get some io up in the air
    scsi_xsh_runqueue(link);

    rv
}

/// `scsi_xsh_del`: takes `xsh` off its link's or its pool's queue; `true` when it was on
/// one.
pub fn scsi_xsh_del(xsh: &ScsiXshandler) -> bool {
    let link = scsi_xsh_link(xsh);
    let pool = link.pool();
    let mut rv = true;

    mtx_enter(&pool.mtx);
    match xsh.ioh.q_state.get() {
        RUNQ_IDLE => rv = false,
        RUNQ_LINKQ => {
            // SAFETY: in state `RUNQ_LINKQ` the handler is on its link's queue; the mutex is
            // held.
            unsafe { link.queue.remove(&xsh.ioh) };
        }
        RUNQ_POOLQ => {
            // SAFETY: in state `RUNQ_POOLQ` the handler is on the pool's queue.
            unsafe { pool.queue.remove(&xsh.ioh) };
            link.pending.set(link.pending.get() - 1);
            if link.state.get() & SDEV_S_DYING != 0 && link.pending.get() == 0 {
                wakeup_one(ptr::from_ref(&link.pending));
            }
        }
        state => panic(format_args!("unexpected xsh state {state}")),
    }
    xsh.ioh.q_state.set(RUNQ_IDLE);
    mtx_leave(&pool.mtx);

    rv
}

/*
 * internal xs runqueue handling.
 */

/// `scsi_xsh_runqueue`: moves the link's waiting transfer handlers to the pool's queue, as
/// many as the link has openings for, and runs the pool.
fn scsi_xsh_runqueue(link: &ScsiLink) {
    let pool = link.pool();

    if !scsi_pending_start(&pool.mtx, &link.running) {
        return;
    }
    loop {
        let mut runq = false;

        mtx_enter(&pool.mtx);
        while link.state.get() & SDEV_S_DYING == 0 && link.pending.get() < link.openings.get() {
            let Some(ioh) = link.queue.first() else {
                break;
            };
            link.pending.set(link.pending.get() + 1);

            // SAFETY: `ioh` is on the link's queue, and whoever queued it keeps it in place
            // until it has left the pool's queue too; the mutex is held.
            unsafe {
                link.queue.remove(ioh);
                pool.queue.insert_tail(ioh);
            }
            ioh.q_state.set(RUNQ_POOLQ);

            runq = true;
        }
        mtx_leave(&pool.mtx);

        if runq {
            scsi_iopool_run(pool);
        }
        if scsi_pending_finish(&pool.mtx, &link.running) {
            break;
        }
    }
}

/// `scsi_xsh_ioh`: the I/O handler of a transfer handler: makes the transfer and calls the
/// device driver's handler with it.
///
/// # Safety
///
/// `cookie` is the `'static` [`ScsiXshandler`] `scsi_xsh_set` paired with this function.
unsafe fn scsi_xsh_ioh(cookie: *mut c_void, io: Option<ScsiIo>) {
    // SAFETY: the caller's contract.
    let xsh: &'static ScsiXshandler = unsafe { &*cookie.cast::<ScsiXshandler>() };
    let Some(io) = io else {
        panic(format_args!("scsi_xsh_ioh: no opening"));
    };

    let Some(xs) = scsi_xs_io(scsi_xsh_link(xsh), io, SCSI_NOSLEEP) else {
        // in this situation we should queue things waiting for an xs and then give them
        // xses when they were supposed be to returned to the pool.
        kprintf!("scsi_xfer pool exhausted!\n");
        scsi_xsh_add(xsh);
        return;
    };

    match xsh.handler.get() {
        Some(handler) => handler(xs),
        None => panic(format_args!("scsi_xshandler {:p} has no handler", xsh)),
    }
}

/// `scsi_xs_get`: a transfer for `link`: an opening of the link, then one of its pool, then
/// a `scsi_xfer` on it. Sleeps for the openings unless `SCSI_NOSLEEP` is in `flags`; `None`
/// when it would have to sleep then, when the link is dying, or when the pool runs out of
/// transfers.
pub fn scsi_xs_get(link: &'static ScsiLink, flags: i32) -> Option<&'static ScsiXfer> {
    if link.state.get() & SDEV_S_DYING != 0 {
        return None;
    }

    let m = ScsiIoMover::new();
    let xsh = ScsiXshandler::new();
    let iopl = link.pool();

    // really custom xs handler to avoid scsi_xsh_ioh
    // SAFETY: `scsi_xs_get_done` takes an `ScsiIoMover`; `m` lives until `scsi_move`
    // returns, which is after the handler has run (or the handler never runs: the request
    // is not queued).
    unsafe {
        scsi_ioh_set(
            &xsh.ioh,
            iopl,
            scsi_xs_get_done,
            ptr::from_ref(&m).cast_mut().cast(),
        );
    }
    xsh.ioh.is_xsh.set(true);
    xsh.link.set(Some(link));

    let io = if !scsi_link_open(link) {
        if flags & SCSI_NOSLEEP != 0 {
            return None;
        }

        // SAFETY: `xsh` stays on this stack until `scsi_move` returns, by which time its
        // handler has run and it is off both queues.
        unsafe { scsi_xsh_add_unchecked(&xsh) };
        scsi_move(&m);
        m.io.get()?
    } else if let Some(io) = scsi_iopool_get(iopl) {
        io
    } else {
        if flags & SCSI_NOSLEEP != 0 {
            scsi_link_close(link);
            return None;
        }

        // SAFETY: as above, for the pool's queue alone.
        unsafe { scsi_ioh_add_unchecked(&xsh.ioh) };
        scsi_move(&m);
        m.io.get()?
    };

    scsi_xs_io(link, io, flags)
}

/// `scsi_xs_get_done`: the handler of `scsi_xs_get`'s waiter.
///
/// # Safety
///
/// As [`scsi_move_done`].
unsafe fn scsi_xs_get_done(cookie: *mut c_void, io: Option<ScsiIo>) {
    // SAFETY: forwarded.
    unsafe { scsi_move_done(cookie, io) }
}

/// `scsi_link_shutdown`: ends every wait for `link`'s openings (the sleepers in
/// `scsi_xs_get` get none) and sleeps until the link's transfers are all back.
pub fn scsi_link_shutdown(link: &ScsiLink) {
    let sleepers = ScsiRunq::new();
    let iopl = link.pool();

    mtx_enter(&iopl.mtx);
    while let Some(ioh) = link.queue.first() {
        // SAFETY: `ioh` is on the link's queue; the pool's mutex is held.
        unsafe { link.queue.remove(ioh) };
        ioh.q_state.set(RUNQ_IDLE);

        if scsi_ioh_is(ioh, scsi_xs_get_done) {
            // SAFETY: just unlinked; a `scsi_xs_get` waiter stays in place until its handler
            // has run, and `sleepers` does not move while it holds elements.
            unsafe { sleepers.insert_tail(ioh) };
        } else {
            #[cfg(feature = "diagnostic")]
            panic(format_args!("scsi_link_shutdown: scsi_xshandler on link"));
        }
    }

    let mut next = iopl.queue.first();
    while let Some(ioh) = next {
        next = ScsiRunq::next(ioh);
        if !ioh.is_xsh.get() {
            continue;
        }
        // SAFETY: `is_xsh` handlers are the `ioh` member, first in the `#[repr(C)]`
        // `ScsiXshandler`, of a live transfer handler.
        let xsh = unsafe { &*ptr::from_ref(ioh).cast::<ScsiXshandler>() };
        let ours = xsh.link.get().is_some_and(|l| ptr::eq(l, link));

        #[cfg(feature = "diagnostic")]
        if ours && scsi_ioh_is(&xsh.ioh, scsi_xsh_ioh) {
            panic(format_args!("scsi_link_shutdown: scsi_xshandler on pool"));
        }

        if ours && scsi_ioh_is(&xsh.ioh, scsi_xs_get_done) {
            // SAFETY: `xsh.ioh` is on the pool's queue; the mutex is held; then it moves to
            // `sleepers` as above.
            unsafe {
                iopl.queue.remove(&xsh.ioh);
                xsh.ioh.q_state.set(RUNQ_IDLE);
                link.pending.set(link.pending.get() - 1);

                sleepers.insert_tail(&xsh.ioh);
            }
        }
    }

    while link.pending.get() > 0 {
        // An uninterruptible sleep without a timeout only ends with the wakeup.
        let _ = msleep_nsec(
            ptr::from_ref(&link.pending),
            &iopl.mtx,
            PRIBIO,
            "pendxs",
            INFSLP,
        );
    }
    mtx_leave(&iopl.mtx);

    while let Some(ioh) = sleepers.first() {
        // SAFETY: `ioh` is on `sleepers`.
        unsafe { sleepers.remove(ioh) };
        scsi_ioh_call(ioh, None);
    }
}

/// `scsi_link_open`: takes one of the link's openings; `false` when they are all in use.
fn scsi_link_open(link: &ScsiLink) -> bool {
    let pool = link.pool();
    let mut open = false;

    mtx_enter(&pool.mtx);
    if link.pending.get() < link.openings.get() {
        link.pending.set(link.pending.get() + 1);
        open = true;
    }
    mtx_leave(&pool.mtx);

    open
}

/// `scsi_link_close`: gives one of the link's openings back.
fn scsi_link_close(link: &ScsiLink) {
    let pool = link.pool();

    mtx_enter(&pool.mtx);
    link.pending.set(link.pending.get() - 1);
    if link.state.get() & SDEV_S_DYING != 0 && link.pending.get() == 0 {
        wakeup_one(ptr::from_ref(&link.pending));
    }
    mtx_leave(&pool.mtx);

    scsi_xsh_runqueue(link);
}

/// `scsi_xs_io`: a transfer for `link` on the opening `io`; without a transfer, gives the
/// opening and the link's slot back.
fn scsi_xs_io(link: &'static ScsiLink, io: ScsiIo, flags: i32) -> Option<&'static ScsiXfer> {
    let wait = if flags & SCSI_NOSLEEP != 0 {
        PR_NOWAIT
    } else {
        PR_WAITOK
    };
    let Some(mem) = pool_get(&SCSI_XFER_POOL, PR_ZERO | wait) else {
        scsi_io_put(link.pool(), io);
        scsi_link_close(link);
        return None;
    };
    let xs = mem.cast::<ScsiXfer>();
    // SAFETY: a fresh pool item of `size_of::<ScsiXfer>()` bytes, suitably aligned, written
    // once before anything else sees it.
    unsafe { xs.as_ptr().write(ScsiXfer::new()) };
    // SAFETY: the item stays allocated until `scsi_xs_put` gives it back.
    let xs: &'static ScsiXfer = unsafe { xs.as_ref() };

    xs.flags.set(flags);
    xs.sc_link.set(Some(link));
    xs.retries.set(SCSI_RETRIES);
    xs.timeout.set(10000);
    xs.io.set(Some(io));

    Some(xs)
}

/// `scsi_xs_put`: gives a transfer, its opening and its link's slot back. `xs` must not be
/// used afterwards.
pub fn scsi_xs_put(xs: &'static ScsiXfer) {
    let link = xs.link();
    let Some(io) = xs.io.get() else {
        panic(format_args!("scsi_xs_put: xs {:p} has no opening", xs));
    };

    pool_put(&SCSI_XFER_POOL, NonNull::from(xs).cast());

    scsi_io_put(link.pool(), io);
    scsi_link_close(link);
}

/// `scsi_test_unit_ready`: sends TEST UNIT READY ("are you ready?").
pub fn scsi_test_unit_ready(
    link: &'static ScsiLink,
    retries: i32,
    flags: i32,
) -> Result<(), Errno> {
    let xs = scsi_xs_get(link, flags).ok_or(ENOMEM)?;
    xs.cmdlen.set(size_of::<ScsiTestUnitReady>() as i32);
    xs.retries.set(retries);
    xs.timeout.set(10000);

    xs.with_cmd(|cmd: &mut ScsiTestUnitReady| cmd.opcode = TEST_UNIT_READY);

    let error = scsi_xs_sync(xs);
    scsi_xs_put(xs);

    error
}

/// `scsi_init_inquiry`: makes `xs` an INQUIRY for `len` bytes of page `pagecode` (with
/// `SI_EVPD` in `flags`) into `data`.
///
/// # Safety
///
/// `data` and `len` meet [`ScsiXfer::set_data`]'s contract.
pub unsafe fn scsi_init_inquiry(xs: &ScsiXfer, flags: u8, pagecode: u8, data: *mut u8, len: usize) {
    xs.with_cmd(|cmd: &mut ScsiInquiry| {
        cmd.opcode = INQUIRY;
        cmd.flags = flags;
        cmd.pagecode = pagecode;
        _lto2b(len as u32, &mut cmd.length);
    });

    xs.cmdlen.set(size_of::<ScsiInquiry>() as i32);

    xs.flags.set(xs.flags.get() | SCSI_DATA_IN);
    // SAFETY: the caller's contract.
    unsafe { xs.set_data(data, len as i32) };
}

/// `scsi_inquire`: asks the device what it is (INQUIRY), first for the 36 basic SCSI-2
/// bytes and then, once, for everything it says it has.
pub fn scsi_inquire(
    link: &'static ScsiLink,
    inqbuf: &mut ScsiInquiryData,
    flags: i32,
) -> Result<(), Errno> {
    // Start by asking for only the basic 36 bytes of SCSI2 inquiry information. This
    // avoids problems with devices that choke trying to supply more.
    let mut bytes = SID_SCSI2_HDRLEN + SID_SCSI2_ALEN;
    let mut retries = 0;

    let (received, avail) = loop {
        let xs = scsi_xs_get(link, flags).ok_or(EBUSY)?;

        bytes = bytes.min(size_of::<ScsiInquiryData>());
        // SAFETY: `inqbuf` is borrowed for the whole call, `bytes` fits in it, and nothing
        // else touches it until `scsi_xs_sync` has returned.
        unsafe { scsi_init_inquiry(xs, 0, 0, inqbuf.as_bytes_mut().as_mut_ptr(), bytes) };

        let error = scsi_xs_sync(xs);
        let received = i64::from(xs.datalen()) - xs.resid.get() as i64;
        scsi_xs_put(xs);

        // SC_DEBUG(link, SDEV_DB2, ("INQUIRE error %d\n", error)): SCSIDEBUG is not
        // configured.
        error?;
        if received < SID_SCSI2_HDRLEN as i64 {
            // SC_DEBUG(link, SDEV_DB2, ("INQUIRE data < SID_SCSI2_HDRLEN\n")).
            return Err(EINVAL);
        }

        let avail = (SID_SCSI2_HDRLEN + usize::from(inqbuf.additional_length)) as i64;

        if received < avail && retries == 0 {
            retries += 1;
            bytes = avail as usize;
            continue;
        }
        break (received, avail);
    };

    // SCSIDEBUG (not configured): sc_print_addr and scsi_show_mem/_inquiry_header/
    // _inquiry_match of the data received.

    if avail > received {
        inqbuf.additional_length = (received - SID_SCSI2_HDRLEN as i64) as u8;
    }

    Ok(())
}

/// `scsi_inquire_vpd`: reads the vital product data page `page` into `buf`.
pub fn scsi_inquire_vpd(
    link: &'static ScsiLink,
    buf: &mut [u8],
    page: u8,
    flags: i32,
) -> Result<(), Errno> {
    if link.flags.get() & SDEV_UMASS != 0 {
        return Err(EJUSTRETURN);
    }

    let xs = scsi_xs_get(link, flags | SCSI_DATA_IN | SCSI_SILENT).ok_or(ENOMEM)?;

    xs.retries.set(2);
    xs.timeout.set(10000);

    // SAFETY: `buf` is borrowed for the whole call and untouched until `scsi_xs_sync` has
    // returned.
    unsafe { scsi_init_inquiry(xs, SI_EVPD, page, buf.as_mut_ptr(), buf.len()) };

    let error = scsi_xs_sync(xs);

    scsi_xs_put(xs);
    // SCSIDEBUG (not configured): how many bytes of the page came back, and their dump.
    error
}

/// `scsi_read_cap_10`: READ CAPACITY (10) into `rdcap`.
pub fn scsi_read_cap_10(
    link: &'static ScsiLink,
    rdcap: &mut ScsiReadCapData,
    flags: i32,
) -> Result<(), Errno> {
    let xs = scsi_xs_get(link, flags | SCSI_DATA_IN | SCSI_SILENT).ok_or(ENOMEM)?;

    let mut cdb = ScsiReadCapacity::zeroed();
    cdb.opcode = READ_CAPACITY;

    xs.set_cmd(&cdb);
    xs.cmdlen.set(size_of::<ScsiReadCapacity>() as i32);
    // SAFETY: `rdcap` is borrowed for the whole call and untouched until `scsi_xs_sync` has
    // returned.
    unsafe {
        xs.set_data(
            rdcap.as_bytes_mut().as_mut_ptr(),
            size_of::<ScsiReadCapData>() as i32,
        )
    };
    xs.timeout.set(20000);

    let rv = scsi_xs_sync(xs);
    scsi_xs_put(xs);

    // SCSIDEBUG (not configured): the dump of the capacity data.

    rv
}

/// `scsi_read_cap_16`: READ CAPACITY (16) into `rdcap`.
pub fn scsi_read_cap_16(
    link: &'static ScsiLink,
    rdcap: &mut ScsiReadCapData16,
    flags: i32,
) -> Result<(), Errno> {
    let xs = scsi_xs_get(link, flags | SCSI_DATA_IN | SCSI_SILENT).ok_or(ENOMEM)?;

    let mut cdb = ScsiReadCapacity16::zeroed();
    cdb.opcode = READ_CAPACITY_16;
    cdb.byte2 = SRC16_SERVICE_ACTION;
    _lto4b(size_of::<ScsiReadCapData16>() as u32, &mut cdb.length);

    xs.set_cmd(&cdb);
    xs.cmdlen.set(size_of::<ScsiReadCapacity16>() as i32);
    // SAFETY: as in `scsi_read_cap_10`.
    unsafe {
        xs.set_data(
            rdcap.as_bytes_mut().as_mut_ptr(),
            size_of::<ScsiReadCapData16>() as i32,
        )
    };
    xs.timeout.set(20000);

    let rv = scsi_xs_sync(xs);
    scsi_xs_put(xs);

    // SCSIDEBUG (not configured): the dump of the capacity data.

    rv
}

/// `scsi_prevent`: prevents or allows (`PR_PREVENT`, `PR_ALLOW`) the removal of the media.
pub fn scsi_prevent(link: &'static ScsiLink, r#type: i32, flags: i32) -> Result<(), Errno> {
    if link.quirks.get() & ADEV_NODOORLOCK != 0 {
        return Ok(());
    }

    let xs = scsi_xs_get(link, flags).ok_or(ENOMEM)?;
    xs.cmdlen.set(size_of::<ScsiPrevent>() as i32);
    xs.retries.set(2);
    xs.timeout.set(5000);

    xs.with_cmd(|cmd: &mut ScsiPrevent| {
        cmd.opcode = PREVENT_ALLOW;
        cmd.how = r#type as u8;
    });

    let error = scsi_xs_sync(xs);
    scsi_xs_put(xs);

    error
}

/// `scsi_start`: sends START STOP UNIT ("start up", `SSS_START`; stop, `SSS_STOP`; eject,
/// `SSS_LOEJ`).
pub fn scsi_start(link: &'static ScsiLink, r#type: i32, flags: i32) -> Result<(), Errno> {
    let xs = scsi_xs_get(link, flags).ok_or(ENOMEM)?;
    xs.cmdlen.set(size_of::<ScsiStartStop>() as i32);
    xs.retries.set(2);
    xs.timeout.set(if r#type == i32::from(SSS_START) {
        30000
    } else {
        10000
    });

    xs.with_cmd(|cmd: &mut ScsiStartStop| {
        cmd.opcode = START_STOP;
        cmd.how = r#type as u8;
    });

    let error = scsi_xs_sync(xs);
    scsi_xs_put(xs);

    error
}

/// `scsi_mode_sense`: MODE SENSE (6) of page `pg_code` into `data`; `EIO` when the reply
/// has no valid header.
pub fn scsi_mode_sense(
    link: &'static ScsiLink,
    pg_code: i32,
    data: &mut ScsiModeSenseBuf,
    flags: i32,
) -> Result<(), Errno> {
    let mut len = size_of::<ScsiModeSenseBuf>();

    let xs = scsi_xs_get(link, flags | SCSI_DATA_IN).ok_or(ENOMEM)?;
    xs.cmdlen.set(size_of::<ScsiModeSense>() as i32);

    // Make sure the sense buffer is clean before we do the mode sense, so that checks for
    // bogus values of 0 will work in case the mode sense fails.
    data.buf.fill(0);

    // SAFETY: `data` is borrowed for the whole call and untouched until `scsi_xs_sync` has
    // returned.
    unsafe { xs.set_data(data.buf.as_mut_ptr(), len as i32) };
    xs.timeout.set(20000);

    len = len.min(0xff);
    xs.with_cmd(|cmd: &mut ScsiModeSense| {
        cmd.opcode = MODE_SENSE;
        cmd.page = pg_code as u8;
        cmd.length = len as u8;
    });

    let mut error = scsi_xs_sync(xs);
    scsi_xs_put(xs);

    if error.is_ok() && !valid_mode_hdr(data.hdr()) {
        error = Err(EIO);
    }

    // SCSIDEBUG (not configured): how many bytes of the page came back, and their dump.

    error
}

/// `scsi_mode_sense_big`: MODE SENSE (10) of page `pg_code` into `data`; `EIO` when the
/// reply has no valid header.
pub fn scsi_mode_sense_big(
    link: &'static ScsiLink,
    pg_code: i32,
    data: &mut ScsiModeSenseBuf,
    flags: i32,
) -> Result<(), Errno> {
    let mut len = size_of::<ScsiModeSenseBuf>();

    let xs = scsi_xs_get(link, flags | SCSI_DATA_IN).ok_or(ENOMEM)?;
    xs.cmdlen.set(size_of::<ScsiModeSenseBig>() as i32);

    // Make sure the sense buffer is clean before we do the mode sense, so that checks for
    // bogus values of 0 will work in case the mode sense fails.
    data.buf.fill(0);

    // SAFETY: as in `scsi_mode_sense`.
    unsafe { xs.set_data(data.buf.as_mut_ptr(), len as i32) };
    xs.timeout.set(20000);

    len = len.min(0xffff);
    xs.with_cmd(|cmd: &mut ScsiModeSenseBig| {
        cmd.opcode = MODE_SENSE_BIG;
        cmd.page = pg_code as u8;
        _lto2b(len as u32, &mut cmd.length);
    });

    let mut error = scsi_xs_sync(xs);
    scsi_xs_put(xs);

    if error.is_ok() && !valid_mode_hdr_big(data.hdr_big()) {
        error = Err(EIO);
    }

    // SCSIDEBUG (not configured): how many bytes of the page came back, and their dump.

    error
}

/// Where the page after a mode parameter header of `header_length` bytes starts, if the
/// reply holds `pg_length` bytes of it and it is page `pg_code`.
fn scsi_mode_page_at(
    buf: &ScsiModeSenseBuf,
    total_length: usize,
    header_length: usize,
    pg_code: i32,
    pg_length: i32,
) -> Option<usize> {
    if (total_length as i64 - header_length as i64) < i64::from(pg_length) {
        return None;
    }

    let page = *buf.buf.get(header_length)?;
    if i32::from(page & SMS_PAGE_CODE) != pg_code {
        return None;
    }

    Some(header_length)
}

/// `scsi_mode_sense_page`: the offset in `buf.buf` of page `pg_code` in a MODE SENSE (6)
/// reply, if the reply holds at least `pg_length` bytes of it.
pub fn scsi_mode_sense_page(buf: &ScsiModeSenseBuf, pg_code: i32, pg_length: i32) -> Option<usize> {
    let hdr = buf.hdr();
    let total_length = usize::from(hdr.data_length) + 1; // sizeof(hdr->data_length)
    let header_length = size_of::<ScsiModeHeader>() + usize::from(hdr.blk_desc_len);

    scsi_mode_page_at(buf, total_length, header_length, pg_code, pg_length)
}

/// `scsi_mode_sense_big_page`: the offset in `buf.buf` of page `pg_code` in a MODE SENSE
/// (10) reply, if the reply holds at least `pg_length` bytes of it.
pub fn scsi_mode_sense_big_page(
    buf: &ScsiModeSenseBuf,
    pg_code: i32,
    pg_length: i32,
) -> Option<usize> {
    let hdr = buf.hdr_big();
    let total_length = _2btol(&hdr.data_length) as usize + 2; // sizeof(hdr->data_length)
    let header_length = size_of::<ScsiModeHeaderBig>() + _2btol(&hdr.blk_desc_len) as usize;

    scsi_mode_page_at(buf, total_length, header_length, pg_code, pg_length)
}

/// `scsi_parse_blkdesc`: the density, block count and block size of the first block
/// descriptor of a mode sense reply (MODE SENSE (10) when `big`), into the outputs that are
/// `Some`; nothing when the reply has no (well-formed) block descriptor.
pub fn scsi_parse_blkdesc(
    link: &ScsiLink,
    buf: &ScsiModeSenseBuf,
    big: bool,
    density: Option<&mut u32>,
    block_count: Option<&mut u64>,
    block_size: Option<&mut u32>,
) {
    let (offset, blk_desc_len) = if !big {
        (
            size_of::<ScsiModeHeader>(),
            u32::from(buf.hdr().blk_desc_len),
        )
    } else {
        (
            size_of::<ScsiModeHeaderBig>(),
            _2btol(&buf.hdr_big().blk_desc_len),
        )
    };

    // Both scsi_blk_desc and scsi_direct_blk_desc are 8 bytes.
    if blk_desc_len == 0 || blk_desc_len % 8 != 0 {
        return;
    }

    match link.inqdata.get().device & SID_TYPE {
        T_SEQUENTIAL => {
            // XXX What other device types return general block descriptors?
            let general: &ScsiBlkDesc = wire_ref(&buf.buf[offset..]);
            if let Some(density) = density {
                *density = u32::from(general.density);
            }
            if let Some(block_size) = block_size {
                *block_size = _3btol(&general.blklen);
            }
            if let Some(block_count) = block_count {
                *block_count = u64::from(_3btol(&general.nblocks));
            }
        }
        _ => {
            let direct: &ScsiDirectBlkDesc = wire_ref(&buf.buf[offset..]);
            if let Some(density) = density {
                *density = u32::from(direct.density);
            }
            if let Some(block_size) = block_size {
                *block_size = _3btol(&direct.blklen);
            }
            if let Some(block_count) = block_count {
                *block_count = u64::from(_4btol(&direct.nblocks));
            }
        }
    }
}

/// `scsi_do_mode_sense`: reads mode page `pg_code` into `buf`, with MODE SENSE (6) and, if
/// that fails and the device may know it, MODE SENSE (10). Returns where the page is in
/// `buf.buf` (`None`: the reply holds less than `pg_length` bytes of it, or another page)
/// and whether the reply is a MODE SENSE (10) one.
pub fn scsi_do_mode_sense(
    link: &'static ScsiLink,
    pg_code: i32,
    buf: &mut ScsiModeSenseBuf,
    pg_length: i32,
    flags: i32,
) -> Result<(Option<usize>, bool), Errno> {
    let mut error = Ok(());

    if link.flags.get() & SDEV_ATAPI == 0 || link.inqdata.get().device & SID_TYPE == T_SEQUENTIAL {
        // Try 6 byte mode sense request first. Some devices don't distinguish between 6
        // and 10 byte MODE SENSE commands, returning 6 byte data for 10 byte requests.
        // ATAPI tape drives use MODE SENSE (6) even though ATAPI uses 10 byte everything
        // else. Don't bother with SMS_DBD. Check returned data length to ensure that at
        // least a header (3 additional bytes) is returned.
        error = scsi_mode_sense(link, pg_code, buf, flags);
        if error.is_ok() {
            // Page data may be invalid (e.g. all zeros) but we accept the device's word that
            // this is the best it can do. Some devices will freak out if their word is not
            // accepted and MODE_SENSE_BIG is attempted.
            return Ok((scsi_mode_sense_page(buf, pg_code, pg_length), false));
        }
    }

    // non-ATAPI, non-USB devices that don't support SCSI-2 commands (i.e. MODE SENSE (10))
    // are done.
    if link.flags.get() & (SDEV_ATAPI | SDEV_UMASS) == 0
        && sid_ansii_rev(&link.inqdata.get()) < SCSI_REV_2
    {
        return error.map(|()| (None, false));
    }

    // Try 10 byte mode sense request.
    scsi_mode_sense_big(link, pg_code, buf, flags)?;

    Ok((scsi_mode_sense_big_page(buf, pg_code, pg_length), true))
}

/// `scsi_mode_select`: MODE SELECT (6) of the parameter list in `data` (a mode header and
/// its pages), whose header's `data_length` gives its length; that byte is zeroed, as the
/// command reserves it.
pub fn scsi_mode_select(
    link: &'static ScsiLink,
    byte2: i32,
    data: &mut [u8],
    flags: i32,
    timeout: i32,
) -> Result<(), Errno> {
    let hdr: &mut ScsiModeHeader = wire_mut(data);
    // 1 == sizeof(data_length); clamped to the buffer (see the module's deviations).
    let len = (usize::from(hdr.data_length) + 1).min(data.len());

    // Length is reserved when doing mode select so zero it.
    wire_mut::<ScsiModeHeader>(data).data_length = 0;

    let xs = scsi_xs_get(link, flags | SCSI_DATA_OUT).ok_or(ENOMEM)?;
    xs.cmdlen.set(size_of::<ScsiModeSelect>() as i32);
    // SAFETY: `data` is borrowed for the whole call and untouched until `scsi_xs_sync` has
    // returned; `len` fits in it.
    unsafe { xs.set_data(data.as_mut_ptr(), len as i32) };
    xs.timeout.set(timeout);

    xs.with_cmd(|cmd: &mut ScsiModeSelect| {
        cmd.opcode = MODE_SELECT;
        cmd.byte2 = byte2 as u8;
        cmd.length = len as u8;
    });

    let error = scsi_xs_sync(xs);
    scsi_xs_put(xs);

    // SC_DEBUG(link, SDEV_DB2, ("scsi_mode_select: error = %d\n", error)).

    error
}

/// `scsi_mode_select_big`: MODE SELECT (10) of the parameter list in `data` (a big mode
/// header and its pages), whose header's `data_length` gives its length; that field is
/// zeroed, as the command reserves it.
pub fn scsi_mode_select_big(
    link: &'static ScsiLink,
    byte2: i32,
    data: &mut [u8],
    flags: i32,
    timeout: i32,
) -> Result<(), Errno> {
    let hdr: &mut ScsiModeHeaderBig = wire_mut(data);
    // 2 == sizeof data_length; clamped to the buffer (see the module's deviations).
    let len = (_2btol(&hdr.data_length) as usize + 2).min(data.len());

    // Length is reserved when doing mode select so zero it.
    _lto2b(0, &mut wire_mut::<ScsiModeHeaderBig>(data).data_length);

    let xs = scsi_xs_get(link, flags | SCSI_DATA_OUT).ok_or(ENOMEM)?;
    xs.cmdlen.set(size_of::<ScsiModeSelectBig>() as i32);
    // SAFETY: as in `scsi_mode_select`.
    unsafe { xs.set_data(data.as_mut_ptr(), len as i32) };
    xs.timeout.set(timeout);

    xs.with_cmd(|cmd: &mut ScsiModeSelectBig| {
        cmd.opcode = MODE_SELECT_BIG;
        cmd.byte2 = byte2 as u8;
        _lto2b(len as u32, &mut cmd.length);
    });

    let error = scsi_xs_sync(xs);
    scsi_xs_put(xs);

    // SC_DEBUG(link, SDEV_DB2, ("scsi_mode_select_big: error = %d\n", error)).

    error
}

/// `scsi_report_luns`: REPORT LUNS (`REPORT_*` in `selectreport`) into the first `datalen`
/// bytes of `data`.
pub fn scsi_report_luns(
    link: &'static ScsiLink,
    selectreport: u8,
    data: &mut ScsiReportLunsData,
    datalen: u32,
    flags: i32,
    timeout: i32,
) -> Result<(), Errno> {
    let datalen = (datalen as usize).min(size_of::<ScsiReportLunsData>());

    let xs = scsi_xs_get(link, flags | SCSI_DATA_IN).ok_or(ENOMEM)?;
    xs.cmdlen.set(size_of::<ScsiReportLuns>() as i32);

    let bytes = data.as_bytes_mut();
    bytes[..datalen].fill(0);
    // SAFETY: `data` is borrowed for the whole call and untouched until `scsi_xs_sync` has
    // returned; `datalen` fits in it.
    unsafe { xs.set_data(bytes.as_mut_ptr(), datalen as i32) };
    xs.timeout.set(timeout);

    xs.with_cmd(|cmd: &mut ScsiReportLuns| {
        cmd.opcode = REPORT_LUNS;
        cmd.selectreport = selectreport;
        _lto4b(datalen as u32, &mut cmd.length);
    });

    let error = scsi_xs_sync(xs);
    scsi_xs_put(xs);

    // SC_DEBUG(link, SDEV_DB2, ("scsi_report_luns: error = %d\n", error)).

    error
}

/// `scsi_xs_exec`: hands `xs` to the adapter; its `scsi_cmd` is responsible for calling
/// [`scsi_done`].
pub fn scsi_xs_exec(xs: &'static ScsiXfer) {
    xs.error.set(XS_NOERROR);
    xs.resid.set(xs.datalen().max(0) as usize);
    xs.status.set(0);
    xs.flags.set(xs.flags.get() & !ITSDONE);

    // SCSIDEBUG (not configured): scsi_show_xs(xs).

    // The adapter's scsi_cmd() is responsible for calling scsi_done().
    kernel_lock();
    (xs.link().bus().adapter().scsi_cmd)(xs);
    kernel_unlock();
}

/// `scsi_copy_internal_data`: for adapters that fake SCSI commands: copies the reply `data`
/// into the transfer's data and sets `resid`.
pub fn scsi_copy_internal_data(xs: &ScsiXfer, data: &[u8]) {
    // SC_DEBUG(xs->sc_link, SDEV_DB3, ("scsi_copy_internal_data\n")).

    if xs.datalen() == 0 {
        sc_print_addr(xs.link());
        kprintf!("uio internal data copy not supported\n");
    } else {
        let datalen = xs.datalen().max(0) as usize;
        let copy_cnt = data.len().min(datalen);
        // SAFETY: `set_data`'s contract: `datalen` bytes at `data()` are this transfer's to
        // write, and `copy_cnt` is no more; the reply is another buffer.
        unsafe { ptr::copy_nonoverlapping(data.as_ptr(), xs.data(), copy_cnt) };
        xs.resid.set(datalen - copy_cnt);
    }
}

/// `scsi_done`: called by the adapter when it is done with `xs`; calls the transfer's
/// `done`.
pub fn scsi_done(xs: &'static ScsiXfer) {
    // SCSIDEBUG (not configured): with SDEV_DB1, scsi_show_mem of the data that came in.

    xs.flags.set(xs.flags.get() | ITSDONE);
    kernel_lock();
    match xs.done.get() {
        Some(done) => done(xs),
        None => panic(format_args!("scsi_done: xs {:p} has no done", xs)),
    }
    kernel_unlock();
}

/// `scsi_xs_sync`: executes `xs` and waits for it (with `SCSI_NOSLEEP`, the adapter polls),
/// retrying while its error says so; returns its errno. `xs` must have no `done` and no
/// `cookie`.
pub fn scsi_xs_sync(xs: &'static ScsiXfer) -> Result<(), Errno> {
    let cookie = Mutex::new(IPL_BIO);
    mtx_init(&cookie, IPL_BIO);

    #[cfg(feature = "diagnostic")]
    {
        if !xs.cookie.get().is_null() {
            panic(format_args!("xs->cookie != NULL in scsi_xs_sync"));
        }
        if xs.done.get().is_some() {
            panic(format_args!("xs->done != NULL in scsi_xs_sync"));
        }
    }

    // If we can't sleep while waiting for completion, get the adapter to complete it for us.
    if xs.flags.get() & SCSI_NOSLEEP != 0 {
        xs.flags.set(xs.flags.get() | SCSI_POLL);
    }

    xs.done.set(Some(scsi_xs_sync_done));

    loop {
        xs.cookie.set(ptr::from_ref(&cookie).cast_mut().cast());

        scsi_xs_exec(xs);

        mtx_enter(&cookie);
        while !xs.cookie.get().is_null() {
            // An uninterruptible sleep without a timeout only ends with the wakeup.
            let _ = msleep_nsec(ptr::from_ref(xs), &cookie, PRIBIO, "syncxs", INFSLP);
        }
        mtx_leave(&cookie);

        let error = scsi_xs_error(xs);
        if error != Err(ERESTART) {
            return error;
        }
    }
}

/// `scsi_xs_sync_done`: the `done` of `scsi_xs_sync`: clears the cookie and wakes the
/// waiter.
fn scsi_xs_sync_done(xs: &'static ScsiXfer) {
    let cookie = xs.cookie.get().cast::<Mutex>().cast_const();

    if cookie.is_null() {
        panic(format_args!("scsi_done called twice on xs({:p})", xs));
    }
    // SAFETY: `scsi_xs_sync` set the cookie to its own mutex, which lives until it has seen
    // the cookie cleared, which it can only do holding the mutex, after we let it go.
    let cookie = unsafe { &*cookie };

    mtx_enter(cookie);
    xs.cookie.set(ptr::null_mut());
    if xs.flags.get() & SCSI_NOSLEEP == 0 {
        wakeup_one(ptr::from_ref(xs));
    }
    mtx_leave(cookie);
}

/// `scsi_xs_error`: the errno for a finished transfer, from its `XS_*` error (and sense);
/// `Err(ERESTART)` asks for a retry while `retries` lasts, then becomes `EIO`.
pub fn scsi_xs_error(xs: &'static ScsiXfer) -> Result<(), Errno> {
    let link = xs.link();

    // SC_DEBUG(xs->sc_link, SDEV_DB3, ("scsi_xs_error,err = 0x%x\n", xs->error)).

    if link.state.get() & SDEV_S_DYING != 0 {
        return Err(ENXIO);
    }

    let error = match xs.error.get() {
        XS_NOERROR => Ok(()), // nearly always hit this one

        XS_SENSE | XS_SHORTSENSE => {
            // SC_DEBUG_SENSE(xs), and the result with SC_DEBUG: SCSIDEBUG is not
            // configured.
            (link.interpret_sense.get())(xs)
        }

        XS_BUSY => scsi_delay(xs, 1),

        XS_TIMEOUT | XS_RESET => Err(ERESTART),

        XS_DRIVER_STUFFUP | XS_SELTIMEOUT => Err(EIO),

        other => {
            sc_print_addr(link);
            kprintf!(
                "unknown error category (0x{:x}) from scsi driver\n",
                other as u32
            );
            Err(EIO)
        }
    };

    if error == Err(ERESTART) {
        let retries = xs.retries.get();
        xs.retries.set(retries - 1);
        if retries < 1 {
            return Err(EIO);
        }
    }
    error
}

/// `scsi_delay`: waits `seconds` before a retry (busy-waits under `SCSI_POLL`, not at all
/// under `SCSI_NOSLEEP`); `Err(ERESTART)` to retry, `EIO` when a signal aborts the wait or
/// the flags make no sense.
pub fn scsi_delay(xs: &ScsiXfer, seconds: i32) -> Result<(), Errno> {
    match xs.flags.get() & (SCSI_POLL | SCSI_NOSLEEP) {
        SCSI_POLL => {
            delay(1_000_000u32.wrapping_mul(seconds as u32));
            return Err(ERESTART);
        }
        SCSI_NOSLEEP => {
            // Retry the command immediately since we can't delay.
            return Err(ERESTART);
        }
        SCSI_AUTOCONF => {
            // Invalid combination!
            return Err(EIO);
        }
        _ => {}
    }

    let ret = tsleep_nsec(
        nowake(),
        PRIBIO | PCATCH,
        "scbusy",
        sec_to_nsec(seconds as u64),
    );

    // Signal == abort xs.
    if ret == Err(ERESTART) || ret == Err(EINTR) {
        return Err(EIO);
    }

    Err(ERESTART)
}

/// `scsi_interpret_sense`: the default error handler: looks at the returned sense and
/// determines the errno to pass back (`Ok`: report no error; `Err(ERESTART)`: retry).
pub fn scsi_interpret_sense(xs: &ScsiXfer) -> Result<(), Errno> {
    let sense = xs.sense.get();
    let link = xs.link();

    // Default sense interpretation.
    let serr = sense.error_code & SSD_ERRCODE;
    let skey = if serr != SSD_ERRCODE_CURRENT && serr != SSD_ERRCODE_DEFERRED {
        0xff // Invalid value, since key is 4 bit value.
    } else {
        sense.flags & SSD_KEY
    };

    // Interpret the key/asc/ascq information where appropriate.
    let error = match skey {
        SKEY_NO_SENSE | SKEY_RECOVERED_ERROR => {
            if xs.resid.get() == xs.datalen().max(0) as usize {
                xs.resid.set(0); // not short read
            }
            Ok(())
        }
        SKEY_BLANK_CHECK | SKEY_EQUAL => Ok(()),
        SKEY_NOT_READY => {
            if xs.flags.get() & SCSI_IGNORE_NOT_READY != 0 {
                return Ok(());
            }
            let mut error = Err(EIO);
            if xs.retries.get() != 0 {
                match asc_ascq(&sense) {
                    SENSE_NOT_READY_BECOMING_READY
                    | SENSE_NOT_READY_FORMAT
                    | SENSE_NOT_READY_REBUILD
                    | SENSE_NOT_READY_RECALC
                    | SENSE_NOT_READY_INPROGRESS
                    | SENSE_NOT_READY_LONGWRITE
                    | SENSE_NOT_READY_SELFTEST
                    | SENSE_NOT_READY_INIT_REQUIRED => {
                        // SC_DEBUG(link, SDEV_DB1, ("not ready (ASC_ASCQ == %#x)\n", ...)).
                        return scsi_delay(xs, 1);
                    }
                    SENSE_NOMEDIUM
                    | SENSE_NOMEDIUM_TCLOSED
                    | SENSE_NOMEDIUM_TOPEN
                    | SENSE_NOMEDIUM_LOADABLE
                    | SENSE_NOMEDIUM_AUXMEM => {
                        link.flags.set(link.flags.get() & !SDEV_MEDIA_LOADED);
                        error = Err(ENOMEDIUM);
                    }
                    _ => {}
                }
            }
            error
        }
        SKEY_MEDIUM_ERROR => match asc_ascq(&sense) {
            SENSE_NOMEDIUM
            | SENSE_NOMEDIUM_TCLOSED
            | SENSE_NOMEDIUM_TOPEN
            | SENSE_NOMEDIUM_LOADABLE
            | SENSE_NOMEDIUM_AUXMEM => {
                link.flags.set(link.flags.get() & !SDEV_MEDIA_LOADED);
                Err(ENOMEDIUM)
            }
            SENSE_BAD_MEDIUM
            | SENSE_NR_MEDIUM_UNKNOWN_FORMAT
            | SENSE_NR_MEDIUM_INCOMPATIBLE_FORMAT
            | SENSE_NW_MEDIUM_UNKNOWN_FORMAT
            | SENSE_NW_MEDIUM_INCOMPATIBLE_FORMAT
            | SENSE_NF_MEDIUM_INCOMPATIBLE_FORMAT
            | SENSE_NW_MEDIUM_AC_MISMATCH => Err(EMEDIUMTYPE),
            _ => Err(EIO),
        },
        SKEY_ILLEGAL_REQUEST => {
            if xs.flags.get() & SCSI_IGNORE_ILLEGAL_REQUEST != 0 {
                return Ok(());
            }
            if asc_ascq(&sense) == SENSE_MEDIUM_REMOVAL_PREVENTED {
                return Err(EBUSY);
            }
            Err(EINVAL)
        }
        SKEY_UNIT_ATTENTION => {
            match asc_ascq(&sense) {
                SENSE_POWER_RESET_OR_BUS
                | SENSE_POWER_ON
                | SENSE_BUS_RESET
                | SENSE_BUS_DEVICE_RESET
                | SENSE_DEVICE_INTERNAL_RESET
                | SENSE_TSC_CHANGE_SE
                | SENSE_TSC_CHANGE_LVD
                | SENSE_IT_NEXUS_LOSS => return scsi_delay(xs, 1),
                _ => {}
            }
            if link.flags.get() & SDEV_REMOVABLE != 0 {
                link.flags.set(link.flags.get() & !SDEV_MEDIA_LOADED);
            }
            if xs.flags.get() & SCSI_IGNORE_MEDIA_CHANGE != 0
                // XXX Should reupload any transient state.
                || link.flags.get() & SDEV_REMOVABLE == 0
            {
                return scsi_delay(xs, 1);
            }
            Err(EIO)
        }
        SKEY_WRITE_PROTECT => Err(EROFS),
        SKEY_ABORTED_COMMAND => Err(ERESTART),
        SKEY_VOLUME_OVERFLOW => Err(ENOSPC),
        SKEY_HARDWARE_ERROR => {
            if asc_ascq(&sense) == SENSE_CARTRIDGE_FAULT {
                return Err(EMEDIUMTYPE);
            }
            Err(EIO)
        }
        _ => Err(EIO),
    };

    // SCSIDEBUG would mean it has already been printed (it is not configured).
    if skey != 0 && xs.flags.get() & SCSI_SILENT == 0 {
        scsi_print_sense(xs);
    }

    error
}

/*
 * Utility routines often used in SCSI stuff
 */

/// `sc_print_addr`: prints the link's address: `sd0(vioblk0:0:0): `, or `probe(...)` before
/// a device driver has the link.
pub fn sc_print_addr(link: &ScsiLink) {
    let adapter_device = link.bus().sc_dev.parent();
    let dev = link.device_softc.get().map(|d| {
        // SAFETY: a device driver stores its own attached device, which detaches (and clears
        // nothing, as in C) before `scsi_detach_lun` frees the link.
        unsafe { d.as_ref() }
    });

    kprintf!(
        "{}({}:{}:{}): ",
        dev.map_or("probe", Device::xname),
        adapter_device.map_or("?", Device::xname),
        link.target.get(),
        link.lun.get()
    );
}

/// `asc2ascii`: a description of the additional sense code `asc` and its qualifier
/// `ascq` into `result` (`SCSITERSE`: just the numbers).
#[cfg(feature = "scsiterse")]
fn asc2ascii(asc: u8, ascq: u8, result: &mut [u8]) {
    snprintf(result, format_args!("ASC 0x{asc:02x} ASCQ 0x{ascq:02x}"));
}

/// `asc2ascii`: a description of the additional sense code `asc` and its qualifier `ascq`
/// into `result`.
#[cfg(not(feature = "scsiterse"))]
fn asc2ascii(asc: u8, ascq: u8, result: &mut [u8]) {
    // Check for a dynamically built description.
    match asc {
        0x40 if ascq >= 0x80 => {
            snprintf(
                result,
                format_args!("Diagnostic Failure on Component 0x{ascq:02x}"),
            );
            return;
        }
        0x4d => {
            snprintf(
                result,
                format_args!("Tagged Overlapped Commands (0x{ascq:02x} = TASK TAG)"),
            );
            return;
        }
        0x70 => {
            snprintf(
                result,
                format_args!("Decompression Exception Short Algorithm ID OF 0x{ascq:02x}"),
            );
            return;
        }
        _ => {}
    }

    // Check for a fixed description.
    if let Some(&(_, _, description)) = ADESC.iter().find(|&&(a, q, _)| a == asc && q == ascq) {
        strlcpy(result, description.as_bytes());
        return;
    }

    // Just print out the ASC and ASCQ values as a description.
    snprintf(result, format_args!("ASC 0x{asc:02x} ASCQ 0x{ascq:02x}"));
}

/// The bytes of a NUL-terminated buffer before the NUL.
fn cstr(buf: &[u8]) -> &[u8] {
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    &buf[..len]
}

/// `scsi_print_sense`: prints the sense data of `xs` (after `sc_print_addr`).
pub fn scsi_print_sense(xs: &ScsiXfer) {
    let sense = xs.sense.get();
    let serr = sense.error_code & SSD_ERRCODE;

    sc_print_addr(xs.link());

    // XXX For error 0x71, current opcode is not the relevant one.
    kprintf!(
        "{}Check Condition (error {}) on opcode 0x{:x}\n",
        if serr == SSD_ERRCODE_DEFERRED {
            "DEFERRED "
        } else {
            ""
        },
        AltHex(u32::from(serr)),
        xs.cmd.get().opcode
    );

    if serr != SSD_ERRCODE_CURRENT && serr != SSD_ERRCODE_DEFERRED {
        if sense.error_code & SSD_ERRCODE_VALID != 0 {
            let usense = ScsiSenseDataUnextended::read_from(sense.as_bytes());
            kprintf!("   AT BLOCK #: {} (decimal)", _3btol(&usense.block) as i32);
        }
        return;
    }

    kprintf!(
        "    SENSE KEY: {}\n",
        Str(cstr(&scsi_decode_sense(&sense, DECODE_SENSE_KEY)))
    );

    if sense.flags & (SSD_FILEMARK | SSD_EOM | SSD_ILI) != 0 {
        let mut pad = ' ';

        kprintf!("             ");
        if sense.flags & SSD_FILEMARK != 0 {
            kprintf!("{pad} Filemark Detected");
            pad = ',';
        }
        if sense.flags & SSD_EOM != 0 {
            kprintf!("{pad} EOM Detected");
            pad = ',';
        }
        if sense.flags & SSD_ILI != 0 {
            kprintf!("{pad} Incorrect Length Indicator Set");
        }
        kprintf!("\n");
    }

    // It is inconvenient to use device type to figure out how to format the info fields.
    // So print them as 32 bit integers.
    let info = _4btol(&sense.info);
    if info != 0 {
        kprintf!(
            "         INFO: 0x{:x} (VALID flag {})\n",
            info,
            if sense.error_code & SSD_ERRCODE_VALID != 0 {
                "on"
            } else {
                "off"
            }
        );
    }

    if sense.extra_len < 4 {
        return;
    }

    let info = _4btol(&sense.cmd_spec_info);
    if info != 0 {
        kprintf!(" COMMAND INFO: 0x{:x}\n", info);
    }
    let sbs = scsi_decode_sense(&sense, DECODE_ASC_ASCQ);
    if !cstr(&sbs).is_empty() {
        kprintf!("     ASC/ASCQ: {}\n", Str(cstr(&sbs)));
    }
    if sense.fru != 0 {
        kprintf!("     FRU CODE: 0x{:x}\n", sense.fru);
    }
    let sbs = scsi_decode_sense(&sense, DECODE_SKSV);
    if !cstr(&sbs).is_empty() {
        kprintf!("         SKSV: {}\n", Str(cstr(&sbs)));
    }
}

/// `scsi_decode_sense`: the text for one part of the sense data (`DECODE_SENSE_KEY`,
/// `DECODE_ASC_ASCQ`, `DECODE_SKSV`), NUL-terminated (empty when there is nothing to say).
fn scsi_decode_sense(sense: &ScsiSenseData, flag: i32) -> [u8; 132] {
    let mut rqsbuf = [0u8; 132];

    let skey = sense.flags & SSD_KEY;
    let spec_1 = sense.sense_key_spec_1;
    let count = _2btol(&[sense.sense_key_spec_2, sense.sense_key_spec_3]) as u16;

    match flag {
        DECODE_SENSE_KEY => {
            strlcpy(&mut rqsbuf, SENSE_KEYS[usize::from(skey)].as_bytes());
        }
        DECODE_ASC_ASCQ => {
            asc2ascii(sense.add_sense_code, sense.add_sense_code_qual, &mut rqsbuf);
        }
        DECODE_SKSV => {
            if sense.extra_len < 9 || spec_1 & SSD_SCS_VALID == 0 {
                return rqsbuf;
            }
            match skey {
                SKEY_ILLEGAL_REQUEST => {
                    let len = snprintf(
                        &mut rqsbuf,
                        format_args!(
                            "Error in {}, Offset {}",
                            if spec_1 & SSD_SCS_CDB_ERROR != 0 {
                                "CDB"
                            } else {
                                "Parameters"
                            },
                            count
                        ),
                    );
                    if len < rqsbuf.len() && spec_1 & SSD_SCS_VALID_BIT_INDEX != 0 {
                        snprintf(
                            &mut rqsbuf[len..],
                            format_args!(", bit {}", spec_1 & SSD_SCS_BIT_INDEX),
                        );
                    }
                }
                SKEY_RECOVERED_ERROR | SKEY_MEDIUM_ERROR | SKEY_HARDWARE_ERROR => {
                    snprintf(&mut rqsbuf, format_args!("Actual Retry Count: {count}"));
                }
                SKEY_NOT_READY => {
                    snprintf(&mut rqsbuf, format_args!("Progress Indicator: {count}"));
                }
                _ => {}
            }
        }
        _ => {}
    }

    rqsbuf
}

/// `scsi_cmd_rw_decode`: the block number and block count of a READ or WRITE CDB (6, 10, 12
/// or 16 bytes), for adapters that emulate SCSI. Panics on any other opcode.
pub fn scsi_cmd_rw_decode(cmd: &ScsiGeneric) -> (u64, u32) {
    match cmd.opcode {
        READ_COMMAND | WRITE_COMMAND => {
            let rw: &ScsiRw = wire_ref(cmd.as_bytes());
            let blkno = u64::from(_3btol(&rw.addr) & (u32::from(SRW_TOPADDR) << 16 | 0xffff));
            let nblks = if rw.length != 0 {
                u32::from(rw.length)
            } else {
                0x100
            };
            (blkno, nblks)
        }
        READ_10 | WRITE_10 => {
            let rw10: &ScsiRw10 = wire_ref(cmd.as_bytes());
            (u64::from(_4btol(&rw10.addr)), _2btol(&rw10.length))
        }
        READ_12 | WRITE_12 => {
            let rw12: &ScsiRw12 = wire_ref(cmd.as_bytes());
            (u64::from(_4btol(&rw12.addr)), _4btol(&rw12.length))
        }
        READ_16 | WRITE_16 => {
            let rw16: &ScsiRw16 = wire_ref(cmd.as_bytes());
            (_8btol(&rw16.addr), _4btol(&rw16.length))
        }
        opcode => panic(format_args!(
            "scsi_cmd_rw_decode: bad opcode 0x{opcode:02x}"
        )),
    }
}

// SCSIDEBUG (not configured): scsidebug_buses,
// scsidebug_targets, scsidebug_luns, scsidebug_level, flagnames, quirknames,
// devicetypenames, scsi_show_sense, scsi_show_xs, scsi_show_mem, scsi_show_flags,
// scsi_show_inquiry_header and scsi_show_inquiry_match.
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `scsi_base.rs`: sense decoding and interpretation, the error and retry
    // logic, the iopool run queue, transfer allocation, and the synchronous commands against a
    // fake adapter that completes every command at once (as an emulating HBA under
    // `SCSI_POLL` does).

    use std::alloc::{Layout, alloc_zeroed};
    use std::boxed::Box;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::string::String;
    use std::sync::MutexGuard;
    use std::vec::Vec;
    use std::{assert, assert_eq, format, vec};

    use super::*;
    use crate::kern::subr_pool::pool_destroy;

    /// What the fake adapter does with the next command.
    enum Reply {
        /// Copies the bytes in (`scsi_copy_internal_data`; nothing for an empty reply).
        Data(Vec<u8>),
        /// Ends the command with this `XS_*` error.
        Error(i32),
        /// Ends the command with `XS_SENSE` and this sense data.
        Sense(ScsiSenseData),
    }

    std::thread_local! {
        /// The fake adapter's script, and the opcodes it was given.
        static FAKE: RefCell<(VecDeque<Reply>, Vec<u8>)> =
            const { RefCell::new((VecDeque::new(), Vec::new())) };
        /// The CDB, `cmdlen`, `timeout` and data length of every command sent.
        static SENT: RefCell<Vec<(Vec<u8>, i32, i32, i32)>> = const { RefCell::new(Vec::new()) };
        /// The cookies of the I/O handlers that got an opening, in order.
        static SERVED: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    }

    /// The fake adapter's `scsi_cmd`: completes `xs` at once with the next scripted reply.
    fn fake_cmd(xs: &'static ScsiXfer) {
        let reply = FAKE.with(|f| {
            let mut f = f.borrow_mut();
            f.1.push(xs.cmd.get().opcode);
            f.0.pop_front()
        });
        SENT.with(|s| {
            let cmd = xs.cmd.get();
            let len = xs.cmdlen.get();
            s.borrow_mut().push((
                cmd.as_bytes()[..len as usize].to_vec(),
                len,
                xs.timeout.get(),
                xs.datalen(),
            ));
        });
        match reply.unwrap_or(Reply::Data(Vec::new())) {
            Reply::Data(d) => {
                if !d.is_empty() {
                    scsi_copy_internal_data(xs, &d);
                }
            }
            Reply::Error(e) => xs.error.set(e),
            Reply::Sense(s) => {
                xs.sense.set(s);
                xs.error.set(XS_SENSE);
            }
        }
        scsi_done(xs);
    }

    static FAKE_ADAPTER: ScsiAdapter = ScsiAdapter {
        scsi_cmd: fake_cmd,
        dev_minphys: None,
        dev_probe: None,
        dev_free: None,
        ioctl: None,
    };

    fn script(replies: Vec<Reply>) {
        FAKE.with(|f| {
            let mut f = f.borrow_mut();
            f.0 = replies.into();
            f.1.clear();
        });
        SENT.with(|s| s.borrow_mut().clear());
    }

    /// The CDB, command length, timeout and data length of each command sent since `script`.
    fn sent() -> Vec<(Vec<u8>, i32, i32, i32)> {
        SENT.with(|s| s.borrow().clone())
    }

    fn opcodes() -> Vec<u8> {
        FAKE.with(|f| f.borrow().1.clone())
    }

    /// A zeroed bus (a softc as `config_attach` hands it out) on the fake adapter.
    fn test_bus() -> &'static ScsibusSoftc {
        // SAFETY: a fresh zeroed allocation of the softc's layout, leaked; all-zero is a valid
        // `ScsibusSoftc` (its `Softc` impl).
        let sb = unsafe { &*alloc_zeroed(Layout::new::<ScsibusSoftc>()).cast::<ScsibusSoftc>() };
        sb.sb_adapter.set(Some(&FAKE_ADAPTER));
        sb
    }

    /// A link with its own default iopool, as `scsi_probe_link` makes one when the adapter has
    /// no pool.
    fn test_link(openings: u16) -> &'static ScsiLink {
        let link: &'static ScsiLink = Box::leak(Box::new(ScsiLink::new()));
        let pool: &'static ScsiIopool = Box::leak(Box::new(ScsiIopool::new()));
        // SAFETY: the default allocator ignores its cookie.
        unsafe {
            scsi_iopool_init(
                pool,
                ptr::from_ref(link).cast_mut().cast(),
                scsi_default_get,
                scsi_default_put,
            );
        }
        link.pool.set(Some(pool));
        link.bus.set(Some(test_bus()));
        link.openings.set(openings);
        link
    }

    /// A transfer outside the pool, for the functions that only read and write its members.
    fn test_xs(link: &'static ScsiLink, flags: i32) -> &'static ScsiXfer {
        let xs: &'static ScsiXfer = Box::leak(Box::new(ScsiXfer::new()));
        xs.sc_link.set(Some(link));
        xs.flags.set(flags);
        xs.retries.set(SCSI_RETRIES);
        xs
    }

    /// Real memory and a fresh `scsi_xfer_pool`.
    fn setup() -> MutexGuard<'static, ()> {
        let g = crate::kern::subr_pool::tests::setup_real_memory();
        pool_init(
            &SCSI_XFER_POOL,
            size_of::<ScsiXfer>(),
            0,
            IPL_BIO,
            0,
            "scxspl",
            None,
        );
        g
    }

    fn teardown() {
        assert_eq!(SCSI_XFER_POOL.pr_nout.get(), 0);
        pool_destroy(&SCSI_XFER_POOL);
    }

    fn decoded(sense: &ScsiSenseData, flag: i32) -> String {
        String::from_utf8_lossy(cstr(&scsi_decode_sense(sense, flag))).into_owned()
    }

    fn sense(key: u8, asc: u8, ascq: u8) -> ScsiSenseData {
        let mut s = ScsiSenseData::new();
        s.error_code = SSD_ERRCODE_CURRENT;
        s.flags = key;
        s.add_sense_code = asc;
        s.add_sense_code_qual = ascq;
        s
    }

    #[test]
    fn asc2ascii_fixed_dynamic_and_unknown() {
        let mut buf = [0u8; 132];
        asc2ascii(0x3a, 0x00, &mut buf);
        assert_eq!(cstr(&buf), b"Medium Not Present");
        asc2ascii(0x40, 0x81, &mut buf);
        assert_eq!(cstr(&buf), b"Diagnostic Failure on Component 0x81");
        asc2ascii(0x4d, 0x07, &mut buf);
        assert_eq!(cstr(&buf), b"Tagged Overlapped Commands (0x07 = TASK TAG)");
        asc2ascii(0x70, 0x2a, &mut buf);
        assert_eq!(
            cstr(&buf),
            b"Decompression Exception Short Algorithm ID OF 0x2a"
        );
        asc2ascii(0xfe, 0xdc, &mut buf);
        assert_eq!(cstr(&buf), b"ASC 0xfe ASCQ 0xdc");
        // A short buffer truncates, as strlcpy and snprintf do.
        let mut small = [0u8; 7];
        asc2ascii(0x3a, 0x00, &mut small);
        assert_eq!(cstr(&small), b"Medium");
    }

    #[test]
    fn decode_sense_parts() {
        let mut s = sense(SKEY_NOT_READY, 0x04, 0x01);
        assert_eq!(decoded(&s, DECODE_SENSE_KEY), "Not Ready");
        assert_eq!(
            decoded(&s, DECODE_ASC_ASCQ),
            "Logical Unit Is in Process Of Becoming Ready"
        );
        // No sense key specific information without SKSV and enough extra bytes.
        assert_eq!(decoded(&s, DECODE_SKSV), "");
        s.extra_len = 10;
        s.sense_key_spec_1 = SSD_SCS_VALID;
        s.sense_key_spec_2 = 0x01;
        s.sense_key_spec_3 = 0x02;
        assert_eq!(decoded(&s, DECODE_SKSV), "Progress Indicator: 258");
        s.flags = SKEY_MEDIUM_ERROR;
        assert_eq!(decoded(&s, DECODE_SKSV), "Actual Retry Count: 258");
        s.flags = SKEY_ILLEGAL_REQUEST;
        s.sense_key_spec_1 = SSD_SCS_VALID | SSD_SCS_CDB_ERROR | SSD_SCS_VALID_BIT_INDEX | 3;
        assert_eq!(decoded(&s, DECODE_SKSV), "Error in CDB, Offset 258, bit 3");
        s.sense_key_spec_1 = SSD_SCS_VALID;
        assert_eq!(decoded(&s, DECODE_SKSV), "Error in Parameters, Offset 258");
        s.flags = SKEY_UNIT_ATTENTION;
        assert_eq!(decoded(&s, DECODE_SKSV), "");
        assert_eq!(decoded(&s, 0), "");
        for (key, name) in SENSE_KEYS.iter().enumerate() {
            s.flags = key as u8;
            assert_eq!(decoded(&s, DECODE_SENSE_KEY), *name);
        }
    }

    #[test]
    fn interpret_sense_maps_keys_to_errnos() {
        let link = test_link(1);
        let xs = test_xs(link, SCSI_SILENT | SCSI_NOSLEEP);
        let check = |s: ScsiSenseData| {
            xs.sense.set(s);
            scsi_interpret_sense(xs)
        };

        // An error code that is neither current nor deferred: no key to go by.
        let mut s = sense(SKEY_NO_SENSE, 0, 0);
        s.error_code = 0x00;
        assert_eq!(check(s), Err(EIO));

        // NO SENSE: not a short read if nothing was transferred.
        let mut buf = [0u8; 8];
        // SAFETY: `buf` outlives the transfer's use of it; nothing else touches it.
        unsafe { xs.set_data(buf.as_mut_ptr(), 8) };
        xs.resid.set(8);
        assert_eq!(check(sense(SKEY_NO_SENSE, 0, 0)), Ok(()));
        assert_eq!(xs.resid.get(), 0);
        xs.clear_data();

        assert_eq!(check(sense(SKEY_BLANK_CHECK, 0, 0)), Ok(()));

        // NOT READY.
        assert_eq!(check(sense(SKEY_NOT_READY, 0x04, 0x01)), Err(ERESTART));
        xs.retries.set(0);
        assert_eq!(check(sense(SKEY_NOT_READY, 0x04, 0x01)), Err(EIO));
        xs.retries.set(1);
        link.flags.set(SDEV_MEDIA_LOADED);
        assert_eq!(check(sense(SKEY_NOT_READY, 0x3a, 0x00)), Err(ENOMEDIUM));
        assert_eq!(link.flags.get() & SDEV_MEDIA_LOADED, 0);
        xs.flags.set(xs.flags.get() | SCSI_IGNORE_NOT_READY);
        assert_eq!(check(sense(SKEY_NOT_READY, 0x3a, 0x00)), Ok(()));
        xs.flags.set(SCSI_SILENT | SCSI_NOSLEEP);

        // MEDIUM ERROR.
        assert_eq!(check(sense(SKEY_MEDIUM_ERROR, 0x3a, 0x02)), Err(ENOMEDIUM));
        assert_eq!(
            check(sense(SKEY_MEDIUM_ERROR, 0x30, 0x00)),
            Err(EMEDIUMTYPE)
        );
        assert_eq!(check(sense(SKEY_MEDIUM_ERROR, 0x11, 0x00)), Err(EIO));

        // ILLEGAL REQUEST.
        assert_eq!(check(sense(SKEY_ILLEGAL_REQUEST, 0x24, 0x00)), Err(EINVAL));
        assert_eq!(check(sense(SKEY_ILLEGAL_REQUEST, 0x53, 0x02)), Err(EBUSY));
        xs.flags.set(xs.flags.get() | SCSI_IGNORE_ILLEGAL_REQUEST);
        assert_eq!(check(sense(SKEY_ILLEGAL_REQUEST, 0x24, 0x00)), Ok(()));
        xs.flags.set(SCSI_SILENT | SCSI_NOSLEEP);

        // UNIT ATTENTION: resets are retried; a media change fails on removable media only.
        assert_eq!(check(sense(SKEY_UNIT_ATTENTION, 0x29, 0x01)), Err(ERESTART));
        assert_eq!(check(sense(SKEY_UNIT_ATTENTION, 0x28, 0x00)), Err(ERESTART));
        link.flags.set(SDEV_REMOVABLE | SDEV_MEDIA_LOADED);
        assert_eq!(check(sense(SKEY_UNIT_ATTENTION, 0x28, 0x00)), Err(EIO));
        assert_eq!(link.flags.get(), SDEV_REMOVABLE);
        xs.flags.set(xs.flags.get() | SCSI_IGNORE_MEDIA_CHANGE);
        assert_eq!(check(sense(SKEY_UNIT_ATTENTION, 0x28, 0x00)), Err(ERESTART));
        xs.flags.set(SCSI_SILENT | SCSI_NOSLEEP);

        assert_eq!(check(sense(SKEY_WRITE_PROTECT, 0, 0)), Err(EROFS));
        assert_eq!(check(sense(SKEY_ABORTED_COMMAND, 0, 0)), Err(ERESTART));
        assert_eq!(check(sense(SKEY_VOLUME_OVERFLOW, 0, 0)), Err(ENOSPC));
        assert_eq!(
            check(sense(SKEY_HARDWARE_ERROR, 0x52, 0x00)),
            Err(EMEDIUMTYPE)
        );
        assert_eq!(check(sense(SKEY_HARDWARE_ERROR, 0x44, 0x00)), Err(EIO));
        assert_eq!(check(sense(SKEY_MISCOMPARE, 0, 0)), Err(EIO));

        // Without SCSI_SILENT the sense is printed (through sc_print_addr) and the result is
        // the same.
        xs.flags.set(SCSI_NOSLEEP);
        let mut s = sense(SKEY_MEDIUM_ERROR, 0x11, 0x00);
        s.extra_len = 10;
        s.info = [0, 0, 0x12, 0x34];
        s.fru = 3;
        assert_eq!(check(s), Err(EIO));
    }

    #[test]
    fn delay_by_flags() {
        let link = test_link(1);
        let xs = test_xs(link, SCSI_NOSLEEP);
        assert_eq!(scsi_delay(xs, 1), Err(ERESTART));
        xs.flags.set(SCSI_AUTOCONF);
        assert_eq!(scsi_delay(xs, 1), Err(EIO));
    }

    #[test]
    fn xs_error_categories_and_retries() {
        let link = test_link(1);
        let xs = test_xs(link, SCSI_SILENT | SCSI_NOSLEEP);

        xs.error.set(XS_NOERROR);
        assert_eq!(scsi_xs_error(xs), Ok(()));
        xs.error.set(XS_DRIVER_STUFFUP);
        assert_eq!(scsi_xs_error(xs), Err(EIO));
        xs.error.set(XS_SELTIMEOUT);
        assert_eq!(scsi_xs_error(xs), Err(EIO));
        xs.error.set(0x77);
        assert_eq!(scsi_xs_error(xs), Err(EIO));

        // ERESTART while retries last, then EIO.
        xs.retries.set(1);
        xs.error.set(XS_TIMEOUT);
        assert_eq!(scsi_xs_error(xs), Err(ERESTART));
        xs.error.set(XS_RESET);
        assert_eq!(scsi_xs_error(xs), Err(EIO));

        // The link's own interpret_sense decides about sense data.
        fn always_busy(_: &'static ScsiXfer) -> Result<(), Errno> {
            Err(EBUSY)
        }
        link.interpret_sense.set(always_busy);
        xs.error.set(XS_SENSE);
        assert_eq!(scsi_xs_error(xs), Err(EBUSY));

        link.state.set(SDEV_S_DYING);
        xs.error.set(XS_NOERROR);
        assert_eq!(scsi_xs_error(xs), Err(ENXIO));
    }

    /// A pool of `free` openings, numbered from 1.
    struct FakeIo {
        free: Cell<usize>,
        next: Cell<usize>,
    }

    unsafe fn fake_get(cookie: *mut c_void) -> Option<ScsiIo> {
        // SAFETY: the cookie is the test's `FakeIo`.
        let f = unsafe { &*cookie.cast::<FakeIo>() };
        if f.free.get() == 0 {
            return None;
        }
        f.free.set(f.free.get() - 1);
        f.next.set(f.next.get() + 1);
        NonNull::new(ptr::without_provenance_mut(f.next.get()))
    }

    unsafe fn fake_put(cookie: *mut c_void, _io: ScsiIo) {
        // SAFETY: as in `fake_get`.
        let f = unsafe { &*cookie.cast::<FakeIo>() };
        f.free.set(f.free.get() + 1);
    }

    /// Records which handler (its cookie) got an opening.
    unsafe fn record(cookie: *mut c_void, io: Option<ScsiIo>) {
        assert!(io.is_some());
        SERVED.with(|s| s.borrow_mut().push(cookie as usize));
    }

    #[test]
    fn iopool_serves_handlers_in_queue_order() {
        let f: &'static FakeIo = Box::leak(Box::new(FakeIo {
            free: Cell::new(0),
            next: Cell::new(0),
        }));
        let pool: &'static ScsiIopool = Box::leak(Box::new(ScsiIopool::new()));
        // SAFETY: `fake_get`/`fake_put` take the `FakeIo`, which is leaked.
        unsafe { scsi_iopool_init(pool, ptr::from_ref(f).cast_mut().cast(), fake_get, fake_put) };
        SERVED.with(|s| s.borrow_mut().clear());

        let iohs: Vec<&'static ScsiIohandler> = (1..=4)
            .map(|i| {
                let ioh: &'static ScsiIohandler = Box::leak(Box::new(ScsiIohandler::new()));
                // SAFETY: `record` reads no cookie, it only records it.
                unsafe { scsi_ioh_set(ioh, pool, record, ptr::without_provenance_mut(i)) };
                ioh
            })
            .collect();

        // Nothing free: they all queue, in order; adding again is a no-op.
        for ioh in &iohs {
            assert!(scsi_ioh_add(ioh));
        }
        assert!(!scsi_ioh_add(iohs[0]));
        assert!(SERVED.with(|s| s.borrow().is_empty()));
        assert_eq!(scsi_io_get(pool, SCSI_NOSLEEP), None);

        // Taking one off the queue skips it.
        assert!(scsi_ioh_del(iohs[1]));
        assert!(!scsi_ioh_del(iohs[1]));

        // Each opening given back goes to the head of the queue.
        let io = NonNull::new(ptr::without_provenance_mut(100)).unwrap_or(SCSI_IOPOOL_POISON);
        scsi_io_put(pool, io);
        assert_eq!(SERVED.with(|s| s.borrow().clone()), vec![1]);
        scsi_io_put(pool, io);
        scsi_io_put(pool, io);
        assert_eq!(SERVED.with(|s| s.borrow().clone()), vec![1, 3, 4]);
        assert!(pool.queue.is_empty());

        // With the queue empty, a returned opening stays free.
        scsi_io_put(pool, io);
        assert_eq!(f.free.get(), 1);
        assert!(scsi_io_get(pool, SCSI_NOSLEEP).is_some());
        assert_eq!(f.free.get(), 0);
    }

    #[test]
    fn pending_start_and_finish() {
        let mtx = Mutex::new(IPL_BIO);
        let running = Cell::new(0);
        assert!(scsi_pending_start(&mtx, &running));
        // A second entry asks the running one for another round.
        assert!(!scsi_pending_start(&mtx, &running));
        assert!(!scsi_pending_finish(&mtx, &running));
        assert_eq!(running.get(), 1);
        assert!(scsi_pending_finish(&mtx, &running));
        assert_eq!(running.get(), 0);
    }

    #[test]
    fn xs_get_and_put_account_for_openings_and_pool_items() {
        let _g = setup();
        let link = test_link(2);

        let xs1 = scsi_xs_get(link, SCSI_NOSLEEP | SCSI_DATA_IN);
        let xs2 = scsi_xs_get(link, SCSI_NOSLEEP);
        let (Some(xs1), Some(xs2)) = (xs1, xs2) else {
            panic!("two openings, two transfers");
        };
        assert_eq!(link.pending.get(), 2);
        assert_eq!(SCSI_XFER_POOL.pr_nout.get(), 2);
        // The link has no third opening, and SCSI_NOSLEEP does not wait for one.
        assert!(scsi_xs_get(link, SCSI_NOSLEEP).is_none());
        assert_eq!(link.pending.get(), 2);

        assert_eq!(xs1.flags.get(), SCSI_NOSLEEP | SCSI_DATA_IN);
        assert_eq!(xs1.retries.get(), SCSI_RETRIES);
        assert_eq!(xs1.timeout.get(), 10000);
        assert_eq!(xs1.io.get(), Some(SCSI_IOPOOL_POISON));
        assert!(ptr::eq(xs1.link(), link));
        assert!(xs1.done.get().is_none() && xs1.cookie.get().is_null());

        scsi_xs_put(xs1);
        assert_eq!(link.pending.get(), 1);
        assert_eq!(SCSI_XFER_POOL.pr_nout.get(), 1);
        scsi_xs_put(xs2);
        assert_eq!(link.pending.get(), 0);

        // A dying link hands out nothing.
        link.state.set(SDEV_S_DYING);
        assert!(scsi_xs_get(link, SCSI_NOSLEEP).is_none());
        link.state.set(0);

        // A link shutdown with nothing outstanding returns at once.
        scsi_link_shutdown(link);
        teardown();
    }

    #[test]
    fn xsh_handler_gets_a_transfer() {
        let _g = setup();
        let link = test_link(1);

        std::thread_local! {
            static GOT: RefCell<Vec<&'static ScsiXfer>> = const { RefCell::new(Vec::new()) };
        }
        fn handler(xs: &'static ScsiXfer) {
            GOT.with(|g| g.borrow_mut().push(xs));
        }
        let xsh: &'static ScsiXshandler = Box::leak(Box::new(ScsiXshandler::new()));
        scsi_xsh_set(xsh, link, handler);

        // The link's one opening is taken: the handler waits on the link.
        let Some(xs) = scsi_xs_get(link, SCSI_NOSLEEP) else {
            panic!("the link has an opening");
        };
        assert!(scsi_xsh_add(xsh));
        assert!(!scsi_xsh_add(xsh));
        assert!(GOT.with(|g| g.borrow().is_empty()));

        // Putting the transfer back frees the opening, and the handler gets a new transfer.
        scsi_xs_put(xs);
        let got = GOT.with(|g| g.borrow_mut().pop());
        let Some(xs) = got else {
            panic!("the handler ran");
        };
        assert_eq!(xs.flags.get(), SCSI_NOSLEEP);
        assert_eq!(link.pending.get(), 1);
        assert!(!scsi_xsh_del(xsh));
        scsi_xs_put(xs);

        // Queued and deleted before an opening came: never runs.
        let Some(xs) = scsi_xs_get(link, SCSI_NOSLEEP) else {
            panic!("the link has an opening");
        };
        assert!(scsi_xsh_add(xsh));
        assert!(scsi_xsh_del(xsh));
        scsi_xs_put(xs);
        assert!(GOT.with(|g| g.borrow().is_empty()));
        assert_eq!(link.pending.get(), 0);
        teardown();
    }

    /// The standard INQUIRY data of a disk, `extra` bytes past the SCSI-2 36.
    fn inquiry_reply(extra: u8, len: usize) -> Vec<u8> {
        let mut d = vec![0u8; len];
        d[0] = T_DIRECT;
        d[2] = SCSI_REV_SPC3;
        d[4] = (SID_SCSI2_ALEN as u8) + extra;
        d[8..16].copy_from_slice(b"VirtIO  ");
        d
    }

    #[test]
    fn sync_commands_against_a_completing_adapter() {
        let _g = setup();
        let link = test_link(1);

        // TEST UNIT READY: one command, no data.
        script(vec![]);
        assert_eq!(
            scsi_test_unit_ready(link, TEST_READY_RETRIES, SCSI_NOSLEEP),
            Ok(())
        );
        assert_eq!(opcodes(), vec![TEST_UNIT_READY]);

        // A timeout is retried; a driver failure is not.
        script(vec![Reply::Error(XS_TIMEOUT), Reply::Data(vec![])]);
        assert_eq!(scsi_test_unit_ready(link, 2, SCSI_NOSLEEP), Ok(()));
        assert_eq!(opcodes(), vec![TEST_UNIT_READY, TEST_UNIT_READY]);
        script(vec![Reply::Error(XS_DRIVER_STUFFUP)]);
        assert_eq!(scsi_test_unit_ready(link, 2, SCSI_NOSLEEP), Err(EIO));
        // Retries run out.
        script(vec![
            Reply::Error(XS_TIMEOUT),
            Reply::Error(XS_TIMEOUT),
            Reply::Error(XS_TIMEOUT),
        ]);
        assert_eq!(scsi_test_unit_ready(link, 1, SCSI_NOSLEEP), Err(EIO));
        assert_eq!(opcodes().len(), 2);
        // Sense data goes through the link's interpret_sense.
        script(vec![Reply::Sense(sense(SKEY_WRITE_PROTECT, 0x27, 0))]);
        assert_eq!(
            scsi_test_unit_ready(link, 1, SCSI_NOSLEEP | SCSI_SILENT),
            Err(EROFS)
        );

        // PREVENT ALLOW, unless the quirk says the door does not lock.
        script(vec![]);
        assert_eq!(
            scsi_prevent(link, i32::from(PR_PREVENT), SCSI_NOSLEEP),
            Ok(())
        );
        assert_eq!(opcodes(), vec![PREVENT_ALLOW]);
        link.quirks.set(ADEV_NODOORLOCK);
        script(vec![]);
        assert_eq!(
            scsi_prevent(link, i32::from(PR_PREVENT), SCSI_NOSLEEP),
            Ok(())
        );
        assert!(opcodes().is_empty());
        link.quirks.set(0);

        // INQUIRY: the basic 36 bytes are enough.
        let mut inq = ScsiInquiryData::new();
        script(vec![Reply::Data(inquiry_reply(0, 36))]);
        assert_eq!(scsi_inquire(link, &mut inq, SCSI_NOSLEEP), Ok(()));
        assert_eq!(opcodes(), vec![INQUIRY]);
        assert_eq!(&inq.vendor, b"VirtIO  ");
        assert_eq!(usize::from(inq.additional_length), SID_SCSI2_ALEN);

        // The device has more: asked for again, once, for everything.
        let mut inq = ScsiInquiryData::new();
        script(vec![
            Reply::Data(inquiry_reply(20, 36)),
            Reply::Data(inquiry_reply(20, 56)),
        ]);
        assert_eq!(scsi_inquire(link, &mut inq, SCSI_NOSLEEP), Ok(()));
        assert_eq!(opcodes(), vec![INQUIRY, INQUIRY]);
        assert_eq!(usize::from(inq.additional_length), SID_SCSI2_ALEN + 20);

        // ... and if it still sends less, the length is cut to what came.
        let mut inq = ScsiInquiryData::new();
        script(vec![
            Reply::Data(inquiry_reply(40, 36)),
            Reply::Data(inquiry_reply(40, 50)),
        ]);
        assert_eq!(scsi_inquire(link, &mut inq, SCSI_NOSLEEP), Ok(()));
        assert_eq!(usize::from(inq.additional_length), 50 - SID_SCSI2_HDRLEN);

        // Too little for a header.
        script(vec![Reply::Data(vec![0; 4])]);
        assert_eq!(scsi_inquire(link, &mut inq, SCSI_NOSLEEP), Err(EINVAL));

        // VPD pages are not asked of UMASS devices.
        let mut vpd = ScsiVpdSerial::default();
        script(vec![Reply::Data(vec![
            0,
            SI_PG_SERIAL,
            0,
            4,
            b'S',
            b'N',
            b'0',
            b'1',
        ])]);
        assert_eq!(
            scsi_inquire_vpd(link, vpd.as_bytes_mut(), SI_PG_SERIAL, SCSI_NOSLEEP),
            Ok(())
        );
        assert_eq!(&vpd.serial[..4], b"SN01");
        link.flags.set(SDEV_UMASS);
        assert_eq!(
            scsi_inquire_vpd(link, vpd.as_bytes_mut(), SI_PG_SERIAL, SCSI_NOSLEEP),
            Err(EJUSTRETURN)
        );
        link.flags.set(0);

        assert_eq!(link.pending.get(), 0);
        teardown();
    }

    #[test]
    fn read_capacity_and_start_stop_send_their_cdbs() {
        let _g = setup();
        let link = test_link(1);

        // READ CAPACITY (10): the data lands in the caller's structure.
        let mut rc = ScsiReadCapData::default();
        script(vec![Reply::Data(vec![0, 0, 0x0f, 0xff, 0, 0, 2, 0])]);
        assert_eq!(scsi_read_cap_10(link, &mut rc, SCSI_NOSLEEP), Ok(()));
        assert_eq!(
            sent(),
            vec![(vec![READ_CAPACITY, 0, 0, 0, 0, 0, 0, 0, 0, 0], 10, 20000, 8)]
        );
        assert_eq!(_4btol(&rc.addr), 0x0fff);
        assert_eq!(_4btol(&rc.length), 512);

        // READ CAPACITY (16): the service action and the allocation length.
        let mut rc16 = ScsiReadCapData16::default();
        let mut reply = vec![0u8; 32];
        reply[7] = 0xff;
        reply[11] = 0x10;
        script(vec![Reply::Data(reply)]);
        assert_eq!(scsi_read_cap_16(link, &mut rc16, SCSI_NOSLEEP), Ok(()));
        let mut cdb = vec![READ_CAPACITY_16, SRC16_SERVICE_ACTION];
        cdb.extend_from_slice(&[0; 8]);
        cdb.extend_from_slice(&[0, 0, 0, 32, 0, 0]);
        assert_eq!(sent(), vec![(cdb, 16, 20000, 32)]);
        assert_eq!(_8btol(&rc16.addr), 0xff);
        assert_eq!(_4btol(&rc16.length), 0x10);

        // A failed transfer comes back as the error.
        script(vec![Reply::Error(XS_DRIVER_STUFFUP)]);
        assert_eq!(scsi_read_cap_10(link, &mut rc, SCSI_NOSLEEP), Err(EIO));

        // START STOP UNIT: the timeout depends on the action.
        script(vec![]);
        assert_eq!(scsi_start(link, i32::from(SSS_START), SCSI_NOSLEEP), Ok(()));
        assert_eq!(scsi_start(link, i32::from(SSS_LOEJ), SCSI_NOSLEEP), Ok(()));
        assert_eq!(
            sent(),
            vec![
                (vec![START_STOP, 0, 0, 0, SSS_START, 0], 6, 30000, 0),
                (vec![START_STOP, 0, 0, 0, SSS_LOEJ, 0], 6, 10000, 0),
            ]
        );

        assert_eq!(link.pending.get(), 0);
        teardown();
    }

    /// A generic command holding the CDB `bytes`.
    fn generic(bytes: &[u8]) -> ScsiGeneric {
        let mut g = ScsiGeneric::zeroed();
        g.as_bytes_mut()[..bytes.len()].copy_from_slice(bytes);
        g
    }

    #[test]
    fn rw_decode_reads_every_cdb_size() {
        // READ (6) and WRITE (6): a 21-bit address, a length of 0 meaning 256.
        for op in [READ_COMMAND, WRITE_COMMAND] {
            assert_eq!(
                scsi_cmd_rw_decode(&generic(&[op, 0xff, 0x12, 0x34, 8, 0])),
                (0x1f1234, 8)
            );
            assert_eq!(
                scsi_cmd_rw_decode(&generic(&[op, 0, 0, 5, 0, 0])),
                (5, 0x100)
            );
        }
        // READ (10) and WRITE (10).
        for op in [READ_10, WRITE_10] {
            assert_eq!(
                scsi_cmd_rw_decode(&generic(&[op, 0, 0x89, 0xab, 0xcd, 0xef, 0, 1, 2, 0])),
                (0x89ab_cdef, 0x0102)
            );
        }
        // READ (12) and WRITE (12).
        for op in [READ_12, WRITE_12] {
            assert_eq!(
                scsi_cmd_rw_decode(&generic(&[op, 0, 0x89, 0xab, 0xcd, 0xef, 1, 2, 3, 4, 0, 0])),
                (0x89ab_cdef, 0x0102_0304)
            );
        }
        // READ (16) and WRITE (16).
        for op in [READ_16, WRITE_16] {
            assert_eq!(
                scsi_cmd_rw_decode(&generic(&[
                    op, 0, 1, 2, 3, 4, 5, 6, 7, 8, 0x0a, 0x0b, 0x0c, 0x0d, 0, 0
                ])),
                (0x0102_0304_0506_0708, 0x0a0b_0c0d)
            );
        }
    }

    /// A MODE SENSE (6) reply: header, one direct-access block descriptor, and `page`.
    fn mode_sense_reply(page: &[u8]) -> Vec<u8> {
        let mut d = vec![0u8, 0, 0, 8];
        d.extend_from_slice(&[0, 0, 0x10, 0, 0, 0, 0x02, 0]); // 4096 blocks of 512 bytes
        d.extend_from_slice(page);
        d[0] = (d.len() - 1) as u8;
        d
    }

    #[test]
    fn mode_sense_finds_the_page_and_the_block_descriptor() {
        let _g = setup();
        let link = test_link(1);
        let mut inq = ScsiInquiryData::new();
        inq.version = SCSI_REV_SPC3;
        link.inqdata.set(inq);

        let page = [
            8u8, 0x12, 0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let mut buf = ScsiModeSenseBuf::new();
        script(vec![Reply::Data(mode_sense_reply(&page))]);
        let r = scsi_do_mode_sense(link, 8, &mut buf, 20 - 4, SCSI_NOSLEEP);
        assert_eq!(r, Ok((Some(12), false)));
        assert_eq!(opcodes(), vec![MODE_SENSE]);
        assert_eq!(buf.buf[12 + 2], 0x04);

        let (mut density, mut count, mut size) = (99u32, 0u64, 0u32);
        scsi_parse_blkdesc(
            link,
            &buf,
            false,
            Some(&mut density),
            Some(&mut count),
            Some(&mut size),
        );
        assert_eq!((density, count, size), (0, 4096, 512));

        // Another page than asked for, or too short a page: no page, but no error.
        script(vec![Reply::Data(mode_sense_reply(&page))]);
        assert_eq!(
            scsi_do_mode_sense(link, 4, &mut buf, 16, SCSI_NOSLEEP),
            Ok((None, false))
        );
        script(vec![Reply::Data(mode_sense_reply(&page))]);
        assert_eq!(
            scsi_do_mode_sense(link, 8, &mut buf, 64, SCSI_NOSLEEP),
            Ok((None, false))
        );

        // MODE SENSE (6) fails: a SCSI-2 device is asked with MODE SENSE (10).
        let mut big = vec![0u8, 0, 0, 0, 0, 0, 0, 0];
        big.extend_from_slice(&page);
        big[1] = (big.len() - 2) as u8;
        script(vec![Reply::Error(XS_DRIVER_STUFFUP), Reply::Data(big)]);
        let r = scsi_do_mode_sense(link, 8, &mut buf, 16, SCSI_NOSLEEP);
        assert_eq!(r, Ok((Some(8), true)));
        assert_eq!(opcodes(), vec![MODE_SENSE, MODE_SENSE_BIG]);

        // ... but a SCSI-1 device is not.
        inq.version = SCSI_REV_1;
        link.inqdata.set(inq);
        script(vec![Reply::Error(XS_DRIVER_STUFFUP)]);
        assert_eq!(
            scsi_do_mode_sense(link, 8, &mut buf, 16, SCSI_NOSLEEP),
            Err(EIO)
        );
        assert_eq!(opcodes(), vec![MODE_SENSE]);

        // A reply without a valid header is an error.
        script(vec![Reply::Data(vec![2, 0, 0, 0])]);
        assert_eq!(scsi_mode_sense(link, 8, &mut buf, SCSI_NOSLEEP), Err(EIO));

        // MODE SELECT sends what the header says and zeroes its length.
        let mut sel = [5u8, 0, 0, 0, 8, 2];
        script(vec![]);
        assert_eq!(
            scsi_mode_select(link, i32::from(SMS_PF), &mut sel, SCSI_NOSLEEP, 1000),
            Ok(())
        );
        assert_eq!(sel[0], 0);
        assert_eq!(opcodes(), vec![MODE_SELECT]);

        assert_eq!(link.pending.get(), 0);
        teardown();
    }

    #[test]
    fn mode_page_offsets_stay_in_the_buffer() {
        let mut buf = ScsiModeSenseBuf::new();
        // The header claims a block descriptor that ends past the buffer.
        buf.buf[0] = 255;
        buf.buf[3] = 250;
        assert_eq!(scsi_mode_sense_page(&buf, 0, 1), None);
        buf.buf[3] = 0;
        buf.buf[4] = 0x45;
        assert_eq!(scsi_mode_sense_page(&buf, 5, 4), Some(4));
        assert_eq!(scsi_mode_sense_page(&buf, 5, 300), None);
        // MODE SENSE (10): header of 8 bytes.
        let mut buf = ScsiModeSenseBuf::new();
        buf.buf[1] = 20;
        buf.buf[8] = 0x08;
        assert_eq!(scsi_mode_sense_big_page(&buf, 8, 10), Some(8));
        buf.buf[6] = 0xff; // blk_desc_len 0xff00
        assert_eq!(scsi_mode_sense_big_page(&buf, 8, 10), None);
    }

    #[test]
    fn alt_hex_is_printf_sharp_x() {
        assert_eq!(format!("{}", AltHex(0)), "0");
        assert_eq!(format!("{}", AltHex(0x70)), "0x70");
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn adesc_matches_the_c_table() {
        let path = crate::reftest::openbsd_src().join("sys/scsi/scsi_base.c");
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let start = text.find("} adesc[] = {").unwrap_or(0);
        let end = text[start..].find("{ 0x00, 0x00, NULL }").unwrap_or(0) + start;
        let mut c = Vec::new();
        for line in text[start..end].lines() {
            let line = line.trim();
            if !line.starts_with("{ 0x") {
                continue;
            }
            let mut parts = line.trim_start_matches('{').splitn(3, ',');
            let asc = parts.next().unwrap_or_default().trim();
            let ascq = parts.next().unwrap_or_default().trim();
            let desc = parts.next().unwrap_or_default().trim();
            let desc = desc.trim_end_matches(',').trim_end_matches('}').trim();
            let desc = desc.trim_matches('"');
            let num = |s: &str| u8::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(0);
            c.push((num(asc), num(ascq), String::from(desc)));
        }
        assert_eq!(c.len(), ADESC.len());
        for (ours, theirs) in ADESC.iter().zip(&c) {
            assert_eq!(
                (ours.0, ours.1, ours.2),
                (theirs.0, theirs.1, theirs.2.as_str())
            );
        }
    }
}
/* </TESTS> */
