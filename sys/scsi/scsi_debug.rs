/*	$OpenBSD: scsi_debug.h,v 1.23 2022/02/28 14:48:11 krw Exp $	*/
/*	$NetBSD: scsi_debug.h,v 1.7 1996/10/12 23:23:16 christos Exp $	*/
/* <LICENSES> */
/*
 * Written by Julian Elischer (julian@tfs.com)
 */
/* </LICENSES> */

/* <CODE> */
//! `<scsi/scsi_debug.h>`: the per-link debugging bits (`SDEV_DB1` .. `SDEV_DB4`, in the
//! `flags` word of the `scsi_link`) and the `SC_DEBUG` macros that test them.
//!
//! Upstream: sys/scsi/scsi_debug.h @ 3ce1f3f79392
//!
//! The file has no licence text; the "Written by" line is kept as it is.
//!
//! ## Deviations
//! - `SCSIDEBUG` is not configured, so only its `#else` branch exists: [`sc_debug!`],
//!   [`sc_debugn!`] and [`sc_debug_sense!`] expand to nothing, as `SC_DEBUG`, `SC_DEBUGN` and
//!   `SC_DEBUG_SENSE` do without the option. Their arguments are not evaluated (the C
//!   macros drop them too). `scsi_base.rs` still writes the call sites as comments, as it
//!   did before this file was ported.
//! - Everything inside `#ifdef SCSIDEBUG` is not ported: the `SCSIDEBUG_BUSES`,
//!   `SCSIDEBUG_TARGETS`, `SCSIDEBUG_LUNS` and `SCSIDEBUG_LEVEL` defaults, the
//!   `scsidebug_*` variables, the `flagnames`, `quirknames` and `devicetypenames` tables and
//!   the `scsi_show_*` functions (declared here, defined in `scsi_base.c`).
//! - The `#ifdef _KERNEL` guard is dropped: the whole tree is the kernel.

/// `SDEV_DB1`: scsi commands, errors, data.
pub const SDEV_DB1: u16 = 0x0010;
/// `SDEV_DB2`: routine flow tracking.
pub const SDEV_DB2: u16 = 0x0020;
/// `SDEV_DB3`: internal to routine flows.
pub const SDEV_DB3: u16 = 0x0040;
/// `SDEV_DB4`: level 4 debugging for this dev.
pub const SDEV_DB4: u16 = 0x0080;

/// `SC_DEBUG(link, level, printstuff)` without `SCSIDEBUG`: nothing.
#[macro_export]
macro_rules! sc_debug {
    ($($arg:tt)*) => {};
}

/// `SC_DEBUGN(link, level, printstuff)` without `SCSIDEBUG`: nothing.
#[macro_export]
macro_rules! sc_debugn {
    ($($arg:tt)*) => {};
}

/// `SC_DEBUG_SENSE(xs)` without `SCSIDEBUG`: nothing.
#[macro_export]
macro_rules! sc_debug_sense {
    ($($arg:tt)*) => {};
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use std::assert_eq;

    use super::*;
    use crate::scsi::scsiconf::SDEV_DBX;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/scsi/scsi_debug.h");
        let ours = crate::reftest::assert_defines!(defs; SDEV_DB1, SDEV_DB2, SDEV_DB3, SDEV_DB4);
        crate::reftest::assert_complete(&defs, "SDEV_", &ours);
    }

    #[test]
    fn the_debug_bits_fill_sdev_dbx() {
        assert_eq!(SDEV_DB1 | SDEV_DB2 | SDEV_DB3 | SDEV_DB4, SDEV_DBX);
    }

    #[test]
    fn the_macros_drop_their_arguments() {
        sc_debug!(link, SDEV_DB2, ("never evaluated {}\n", undefined_name));
        sc_debugn!(link, SDEV_DB2, ("never evaluated\n"));
        sc_debug_sense!(xs);
    }
}
/* </TESTS> */
