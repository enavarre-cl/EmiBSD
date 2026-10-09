/* <LICENSES> */
/*
 * Copyright (c) 1994 Charles Hannum.  All rights reserved.
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
 *	This product includes software developed by Charles Hannum.
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
 * Contributed by HD Associates (hd@world.std.com).
 * Copyright (c) 1992, 1993 HD Associates
 *
 * Berkeley style copyright.
 */
/* </LICENSES> */

/* <CODE> */
//! The SCSI midlayer's ioctls, which a device driver (`sdioctl`, `cdioctl`, ...) hands on
//! for the commands it does not know: a device's address (`SCIOCIDENTIFY`), raw SCSI
//! commands (`SCIOCCOMMAND`), ATA pass-through (`ATAIOCCOMMAND`) and the debug level
//! (`SCIOCDEBUG`); anything else goes to the adapter's `ioctl`.
//!
//! Upstream: sys/scsi/scsi_ioctl.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `caddr_t addr` is the kernel copy of the argument as a byte slice, as `d_ioctl` gets it
//!   (`sys/conf.rs`); the request structures are read out of it and written back into it
//!   (`ioctl_arg`, `ioctl_ret`), where the C changes them in place. The adapter's `ioctl`
//!   still gets the raw pointer its type takes.
//! - `dma_alloc(9)` (`kern/dma_alloc.c`) is not ported: the data buffers come from
//!   `malloc(9)` through `scsiconf.rs`'s `DmaBuf`, freed when it is dropped, after the
//!   transfer has been given back (`scsi_xs_put`), as the C frees them before.
//! - `SCSIDEBUG` is not configured: the `SC_DEBUG`/`SC_DEBUG_SENSE` sites are comments.
//!   `SCIOCDEBUG` still sets the link's `SDEV_DB1`..`SDEV_DB4` bits, as in C.
//! - `ATAIOCCOMMAND`'s `atareq_t` comes from `sys/ataio.rs`, ported for this file.
//! - The `DIAGNOSTIC` panic for an impossible command in the second `switch` is behind the
//!   `diagnostic` feature, as in C.

use core::mem::size_of;

use crate::kprintf;
use crate::machine::copy::{copyin, copyout};
use crate::scsi::scsi_all::{
    ATA_PASSTHRU_12, ATA_PASSTHRU_PROTO_NON_DATA, ATA_PASSTHRU_PROTO_PIO_DATAIN,
    ATA_PASSTHRU_PROTO_PIO_DATAOUT, ATA_PASSTHRU_T_DIR_READ, ATA_PASSTHRU_T_DIR_WRITE,
    ATA_PASSTHRU_T_LEN_NONE, ATA_PASSTHRU_T_LEN_SECTOR_COUNT, ScsiAtaPassthru12, ScsiGeneric,
    ScsiSenseData, ScsiWire,
};
use crate::scsi::scsi_base::{scsi_xs_get, scsi_xs_put, scsi_xs_sync};
use crate::scsi::scsi_debug::{SDEV_DB1, SDEV_DB2, SDEV_DB3, SDEV_DB4};
use crate::scsi::scsiconf::{
    DmaBuf, SCSI_DATA_IN, SCSI_DATA_OUT, SCSI_SILENT, SDEV_ATAPI, SDEV_DBX, SDEV_UMASS, ScsiLink,
    ScsiXfer, XS_BUSY, XS_DRIVER_STUFFUP, XS_NOERROR, XS_SENSE, XS_SHORTSENSE, XS_TIMEOUT,
};
use crate::sys::ataio::{
    ATACMD_ERROR, ATACMD_OK, ATACMD_READ, ATACMD_WRITE, ATAIOCCOMMAND, Atareq,
};
use crate::sys::errno::Errno::{self, *};
use crate::sys::fcntl::FWRITE;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::M_WAITOK;
use crate::sys::param::MAXPHYS;
use crate::sys::scsiio::{
    SCCMD_BUSY, SCCMD_OK, SCCMD_READ, SCCMD_SENSE, SCCMD_TIMEOUT, SCCMD_UNKNOWN, SCCMD_WRITE,
    SCIOCCOMMAND, SCIOCDEBUG, SCIOCIDENTIFY, SENSEBUFLEN, ScsiAddr, Scsireq, TYPE_ATAPI, TYPE_SCSI,
};

/// Builds `scsi_readsafe_cmd[]` from the opcodes that only read.
const fn readsafe(opcodes: &[u8]) -> [bool; 256] {
    let mut t = [false; 256];
    let mut i = 0;
    while i < opcodes.len() {
        t[opcodes[i] as usize] = true;
        i += 1;
    }
    t
}

/// `scsi_readsafe_cmd[]`: the commands `SCIOCCOMMAND` allows without `FWRITE`.
pub const SCSI_READSAFE_CMD: [bool; 256] = readsafe(&[
    0x00, // TEST UNIT READY
    0x03, // REQUEST SENSE
    0x08, // READ(6)
    0x12, // INQUIRY
    0x1a, // MODE SENSE
    0x1b, // START STOP
    0x23, // READ FORMAT CAPACITIES
    0x25, // READ CDVD CAPACITY
    0x28, // READ(10)
    0x2b, // SEEK
    0x2f, // VERIFY(10)
    0x3c, // READ BUFFER
    0x3e, // READ LONG
    0x42, // READ SUBCHANNEL
    0x43, // READ TOC PMA ATIP
    0x44, // READ HEADER
    0x45, // PLAY AUDIO(10)
    0x46, // GET CONFIGURATION
    0x47, // PLAY AUDIO MSF
    0x48, // PLAY AUDIO TI
    0x4a, // GET EVENT STATUS NOTIFICATION
    0x4b, // PAUSE RESUME
    0x4e, // STOP PLAY SCAN
    0x51, // READ DISC INFO
    0x52, // READ TRACK RZONE INFO
    0x5a, // MODE SENSE(10)
    0x88, // READ(16)
    0x8f, // VERIFY(16)
    0xa4, // REPORT KEY
    0xa5, // PLAY AUDIO(12)
    0xa8, // READ(12)
    0xac, // GET PERFORMANCE
    0xad, // READ DVD STRUCTURE
    0xb9, // READ CD MSF
    0xba, // SCAN
    0xbc, // PLAY CD
    0xbd, // MECHANISM STATUS
    0xbe, // READ CD
]);

/// Gives `xs` its data buffer: `len` zeroed bytes (`dma_alloc(len, PR_WAITOK | PR_ZERO)`),
/// or none for 0. `Err(ENOMEM)` when the allocation fails.
fn xs_data(xs: &ScsiXfer, len: usize) -> Result<Option<DmaBuf>, Errno> {
    if len == 0 {
        return Ok(None);
    }
    let mut buf = DmaBuf::new(len, M_WAITOK).ok_or(ENOMEM)?;
    let bytes = buf.bytes();
    // SAFETY: the buffer is the caller's until it drops it, which it does only after
    // `scsi_xs_put` (or `clear_data`); nothing else touches it meanwhile. `len <= MAXPHYS`
    // fits an `int`.
    unsafe { xs.set_data(bytes.as_mut_ptr(), bytes.len() as i32) };
    Ok(Some(buf))
}

/// `scsi_ioc_cmd`: runs the raw command `screq` on `link`.
fn scsi_ioc_cmd(link: &'static ScsiLink, screq: &mut Scsireq) -> Result<(), Errno> {
    let cmdlen = usize::from(screq.cmdlen);
    if cmdlen > size_of::<ScsiGeneric>() {
        return Err(EFAULT);
    }
    if screq.datalen > MAXPHYS as u64 {
        return Err(EINVAL);
    }
    let datalen = screq.datalen as usize;

    let xs = scsi_xs_get(link, 0).ok_or(ENOMEM)?;

    xs.with_cmd(|cmd: &mut ScsiGeneric| {
        cmd.as_bytes_mut()[..cmdlen].copy_from_slice(&screq.cmd[..cmdlen]);
    });
    xs.cmdlen.set(cmdlen as i32);

    let mut data = match xs_data(xs, datalen) {
        Ok(d) => d,
        Err(e) => {
            scsi_xs_put(xs);
            return Err(e);
        }
    };
    let err = 'err: {
        if screq.flags & SCCMD_READ != 0 {
            xs.flags.set(xs.flags.get() | SCSI_DATA_IN);
        }
        if screq.flags & SCCMD_WRITE != 0 {
            if let Some(buf) = data.as_mut()
                && let Err(e) = copyin(screq.databuf, buf.bytes())
            {
                break 'err Err(e);
            }
            xs.flags.set(xs.flags.get() | SCSI_DATA_OUT);
        }

        // User is responsible for errors.
        xs.flags.set(xs.flags.get() | SCSI_SILENT);
        xs.timeout.set(screq.timeout as i32);
        xs.retries.set(0); // user must do the retries; ignored

        // The C ignores the result: the outcome is in xs->error.
        let _ = scsi_xs_sync(xs);

        screq.retsts = 0;
        screq.status = xs.status.get();
        match xs.error.get() {
            XS_NOERROR => {
                // probably rubbish
                screq.datalen_used = (xs.datalen() as u64).wrapping_sub(xs.resid.get() as u64);
                screq.retsts = SCCMD_OK;
            }
            XS_SENSE => {
                // SC_DEBUG_SENSE(xs): SCSIDEBUG is not configured.
                copy_sense(xs, screq);
                screq.retsts = SCCMD_SENSE;
            }
            XS_SHORTSENSE => {
                // SC_DEBUG_SENSE(xs): SCSIDEBUG is not configured.
                kprintf!("XS_SHORTSENSE\n");
                copy_sense(xs, screq);
                screq.retsts = SCCMD_UNKNOWN;
            }
            XS_DRIVER_STUFFUP => screq.retsts = SCCMD_UNKNOWN,
            XS_TIMEOUT => screq.retsts = SCCMD_TIMEOUT,
            XS_BUSY => screq.retsts = SCCMD_BUSY,
            _ => screq.retsts = SCCMD_UNKNOWN,
        }

        if screq.flags & SCCMD_READ != 0
            && let Some(buf) = data.as_mut()
        {
            break 'err copyout(buf.bytes(), screq.databuf);
        }
        Ok(())
    };

    xs.clear_data();
    scsi_xs_put(xs);
    drop(data);

    err
}

/// Copies `xs`'s sense data into `screq` (`senselen_used = min(sizeof(xs->sense),
/// sizeof(screq->sense))`).
fn copy_sense(xs: &ScsiXfer, screq: &mut Scsireq) {
    let n = size_of::<ScsiSenseData>().min(SENSEBUFLEN);
    screq.senselen_used = n as u8;
    screq.sense[..n].copy_from_slice(&xs.sense.get().as_bytes()[..n]);
}

/// `scsi_ioc_ata_cmd`: runs the ATA command `atareq` on `link` through ATA PASS-THROUGH(12).
fn scsi_ioc_ata_cmd(link: &'static ScsiLink, atareq: &mut Atareq) -> Result<(), Errno> {
    if atareq.datalen > MAXPHYS as u64 {
        return Err(EINVAL);
    }
    let datalen = atareq.datalen as usize;

    let xs = scsi_xs_get(link, 0).ok_or(ENOMEM)?;

    xs.with_cmd(|cdb: &mut ScsiAtaPassthru12| {
        cdb.opcode = ATA_PASSTHRU_12;

        if datalen > 0 {
            if atareq.flags & ATACMD_READ != 0 {
                cdb.count_proto = ATA_PASSTHRU_PROTO_PIO_DATAIN;
                cdb.flags = ATA_PASSTHRU_T_DIR_READ;
            } else {
                cdb.count_proto = ATA_PASSTHRU_PROTO_PIO_DATAOUT;
                cdb.flags = ATA_PASSTHRU_T_DIR_WRITE;
            }
            cdb.flags |= ATA_PASSTHRU_T_LEN_SECTOR_COUNT;
        } else {
            cdb.count_proto = ATA_PASSTHRU_PROTO_NON_DATA;
            cdb.flags = ATA_PASSTHRU_T_LEN_NONE;
        }
        cdb.features = atareq.features;
        cdb.sector_count = atareq.sec_count;
        cdb.lba_low = atareq.sec_num;
        cdb.lba_mid = atareq.cylinder as u8;
        cdb.lba_high = (atareq.cylinder >> 8) as u8;
        cdb.device = atareq.head & 0x0f;
        cdb.command = atareq.command;
    });

    xs.cmdlen.set(size_of::<ScsiAtaPassthru12>() as i32);

    let mut data = match xs_data(xs, datalen) {
        Ok(d) => d,
        Err(e) => {
            scsi_xs_put(xs);
            return Err(e);
        }
    };
    let err = 'err: {
        if atareq.flags & ATACMD_READ != 0 {
            xs.flags.set(xs.flags.get() | SCSI_DATA_IN);
        }
        if atareq.flags & ATACMD_WRITE != 0 {
            if let Some(buf) = data.as_mut()
                && let Err(e) = copyin(atareq.databuf, buf.bytes())
            {
                break 'err Err(e);
            }
            xs.flags.set(xs.flags.get() | SCSI_DATA_OUT);
        }

        // User is responsible for errors.
        xs.flags.set(xs.flags.get() | SCSI_SILENT);
        xs.retries.set(0); // user must do the retries; ignored

        // The C ignores the result: the outcome is in xs->error.
        let _ = scsi_xs_sync(xs);

        atareq.retsts = match xs.error.get() {
            // SC_DEBUG_SENSE(xs) for the sense cases (SCSIDEBUG is not configured); the C
            // says "XXX this is not right" and falls through to XS_NOERROR.
            XS_SENSE | XS_SHORTSENSE | XS_NOERROR => ATACMD_OK,
            _ => ATACMD_ERROR,
        };

        if atareq.flags & ATACMD_READ != 0
            && let Some(buf) = data.as_mut()
        {
            break 'err copyout(buf.bytes(), atareq.databuf);
        }
        Ok(())
    };

    xs.clear_data();
    scsi_xs_put(xs);
    drop(data);

    err
}

/// `scsi_do_ioctl`: something (e.g. another driver) has called us with a link for a
/// target/LUN/adapter and a SCSI-specific ioctl to perform; `addr` is the kernel copy of
/// the argument (`d_ioctl`'s `data`), `flag` the open file's flags (`FWRITE`).
pub fn scsi_do_ioctl(
    link: &'static ScsiLink,
    cmd: u64,
    addr: &mut [u8],
    flag: i32,
) -> Result<(), Errno> {
    // SC_DEBUG(link, SDEV_DB2, ("scsi_do_ioctl(0x%lx)\n", cmd)): SCSIDEBUG is not configured.

    match cmd {
        SCIOCIDENTIFY => {
            let sca = ScsiAddr {
                r#type: if link.flags.get() & (SDEV_ATAPI | SDEV_UMASS) == 0 {
                    // A 'real' SCSI target.
                    TYPE_SCSI
                } else {
                    // An 'emulated' SCSI target.
                    TYPE_ATAPI
                },
                scbus: link.bus().sc_dev.dv_unit.get(),
                target: i32::from(link.target.get()),
                lun: i32::from(link.lun.get()),
            };
            ioctl_ret(addr, &sca);
            return Ok(());
        }
        SCIOCCOMMAND | ATAIOCCOMMAND | SCIOCDEBUG => {
            let readsafe = cmd == SCIOCCOMMAND
                && SCSI_READSAFE_CMD[usize::from(ioctl_arg::<Scsireq>(addr).cmd[0])];
            if !readsafe && flag & FWRITE == 0 {
                return Err(EPERM);
            }
        }
        _ => {
            return match link.bus().adapter().ioctl {
                // SAFETY: `addr` is the kernel copy of the command's argument that `d_ioctl`
                // received, the contract of the adapter's `ioctl`.
                Some(ioctl) => unsafe { ioctl(link, cmd, addr.as_mut_ptr(), flag) },
                None => Err(ENOTTY),
            };
        }
    }

    match cmd {
        SCIOCCOMMAND => {
            let mut screq = ioctl_arg::<Scsireq>(addr);
            let r = scsi_ioc_cmd(link, &mut screq);
            ioctl_ret(addr, &screq);
            r
        }
        ATAIOCCOMMAND => {
            let mut atareq = ioctl_arg::<Atareq>(addr);
            let r = scsi_ioc_ata_cmd(link, &mut atareq);
            ioctl_ret(addr, &atareq);
            r
        }
        SCIOCDEBUG => {
            let level = ioctl_arg::<i32>(addr);

            // SC_DEBUG(link, SDEV_DB3, ("debug set to %d\n", level)): not configured.
            let mut flags = link.flags.get() & !SDEV_DBX; // clear debug bits
            for (bit, db) in [(1, SDEV_DB1), (2, SDEV_DB2), (4, SDEV_DB3), (8, SDEV_DB4)] {
                if level & bit != 0 {
                    flags |= db;
                }
            }
            link.flags.set(flags);
            Ok(())
        }
        _ => {
            #[cfg(feature = "diagnostic")]
            crate::kern::subr_prf::panic(format_args!("scsi_do_ioctl: impossible cmd ({cmd:#x})"));
            #[cfg(not(feature = "diagnostic"))]
            Ok(())
        }
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `scsi_ioctl.rs`: the read-safe table, the permission checks, and the
    // commands without user data against an adapter that completes at once.

    use std::alloc::{Layout, alloc_zeroed};
    use std::boxed::Box;
    use std::sync::MutexGuard;
    use std::{assert, assert_eq};

    use super::*;
    use crate::kern::subr_pool::{pool_destroy, pool_init};
    use crate::machine::intr::IPL_BIO;
    use crate::scsi::scsi_all::{SKEY_ILLEGAL_REQUEST, SSD_ERRCODE_CURRENT};
    use crate::scsi::scsi_base::{
        SCSI_XFER_POOL, scsi_default_get, scsi_default_put, scsi_done, scsi_iopool_init,
    };
    use crate::scsi::scsiconf::{
        SDEV_NO_ADAPTER_TARGET, ScsiAdapter, ScsiIopool, ScsibusSoftc, XS_SELTIMEOUT,
    };
    use crate::sys::ioctl::{ioctl_arg, ioctl_ret};

    /// Completes every command at once: TEST UNIT READY (0x00) fine, REQUEST SENSE (0x03) with
    /// sense data, anything else with a selection timeout.
    fn fake_cmd(xs: &'static ScsiXfer) {
        match xs.cmd.get().opcode {
            0x00 => xs.status.set(0),
            0x03 => {
                let mut sense = ScsiSenseData::default();
                sense.error_code = SSD_ERRCODE_CURRENT;
                sense.flags = SKEY_ILLEGAL_REQUEST;
                xs.sense.set(sense);
                xs.error.set(XS_SENSE);
            }
            _ => xs.error.set(XS_SELTIMEOUT),
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

    /// Real memory, the transfer pool, and a link (target 2, LUN 1) on a bus of the fake adapter.
    fn setup() -> (MutexGuard<'static, ()>, &'static ScsiLink) {
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
        // SAFETY: a fresh zeroed allocation of the softc's layout, leaked; all-zero is a valid
        // `ScsibusSoftc` (its `Softc` impl).
        let sb = unsafe { &*alloc_zeroed(Layout::new::<ScsibusSoftc>()).cast::<ScsibusSoftc>() };
        sb.sb_adapter.set(Some(&FAKE_ADAPTER));
        sb.sb_adapter_target.set(SDEV_NO_ADAPTER_TARGET);
        sb.sc_dev.dv_unit.set(3);

        let link: &'static ScsiLink = Box::leak(Box::new(ScsiLink::new()));
        let pool: &'static ScsiIopool = Box::leak(Box::new(ScsiIopool::new()));
        // SAFETY: the default allocator ignores its cookie.
        unsafe {
            scsi_iopool_init(
                pool,
                core::ptr::from_ref(link).cast_mut().cast(),
                scsi_default_get,
                scsi_default_put,
            );
        }
        link.pool.set(Some(pool));
        link.bus.set(Some(sb));
        link.openings.set(1);
        link.target.set(2);
        link.lun.set(1);
        (g, link)
    }

    fn teardown() {
        assert_eq!(SCSI_XFER_POOL.pr_nout.get(), 0);
        pool_destroy(&SCSI_XFER_POOL);
    }

    #[test]
    fn readsafe_table_lists_the_reading_commands() {
        let n = SCSI_READSAFE_CMD.iter().filter(|&&b| b).count();
        assert_eq!(n, 38);
        assert!(SCSI_READSAFE_CMD[0x28] && SCSI_READSAFE_CMD[0x12] && SCSI_READSAFE_CMD[0xbe]);
        assert!(!SCSI_READSAFE_CMD[0x2a] && !SCSI_READSAFE_CMD[0x0a]);
    }

    #[test]
    fn identify_reports_the_address() {
        let (_g, link) = setup();
        let mut buf = [0u8; 16];
        assert_eq!(scsi_do_ioctl(link, SCIOCIDENTIFY, &mut buf, 0), Ok(()));
        let sca: ScsiAddr = ioctl_arg(&buf);
        assert_eq!(
            sca,
            ScsiAddr {
                r#type: TYPE_SCSI,
                scbus: 3,
                target: 2,
                lun: 1
            }
        );
        link.flags.set(SDEV_UMASS);
        assert_eq!(scsi_do_ioctl(link, SCIOCIDENTIFY, &mut buf, 0), Ok(()));
        assert_eq!(ioctl_arg::<ScsiAddr>(&buf).r#type, TYPE_ATAPI);
        teardown();
    }

    #[test]
    fn writing_commands_need_fwrite() {
        let (_g, link) = setup();
        let mut buf = [0u8; size_of::<Scsireq>()];
        let mut screq = Scsireq::new();
        screq.cmd[0] = 0x2a; // WRITE(10)
        screq.cmdlen = 10;
        ioctl_ret(&mut buf, &screq);
        assert_eq!(scsi_do_ioctl(link, SCIOCCOMMAND, &mut buf, 0), Err(EPERM));

        let mut lvl = [0u8; 4];
        ioctl_ret(&mut lvl, &5i32);
        assert_eq!(scsi_do_ioctl(link, SCIOCDEBUG, &mut lvl, 0), Err(EPERM));
        let mut abuf = [0u8; size_of::<Atareq>()];
        assert_eq!(scsi_do_ioctl(link, ATAIOCCOMMAND, &mut abuf, 0), Err(EPERM));

        // Unknown commands go to the adapter, which has no ioctl.
        assert_eq!(
            scsi_do_ioctl(link, 0x2000_5107, &mut lvl, FWRITE),
            Err(ENOTTY)
        );
        teardown();
    }

    #[test]
    fn debug_level_sets_the_debug_bits() {
        let (_g, link) = setup();
        link.flags.set(SDEV_UMASS | SDEV_DBX);
        let mut lvl = [0u8; 4];
        ioctl_ret(&mut lvl, &0b0101i32);
        assert_eq!(scsi_do_ioctl(link, SCIOCDEBUG, &mut lvl, FWRITE), Ok(()));
        assert_eq!(link.flags.get(), SDEV_UMASS | 0x0010 | 0x0040);
        teardown();
    }

    #[test]
    fn raw_commands_return_status_and_sense() {
        let (_g, link) = setup();
        let mut buf = [0u8; size_of::<Scsireq>()];

        // TEST UNIT READY is read-safe: no FWRITE needed.
        let mut screq = Scsireq::new();
        screq.cmdlen = 6;
        screq.timeout = 1000;
        ioctl_ret(&mut buf, &screq);
        assert_eq!(scsi_do_ioctl(link, SCIOCCOMMAND, &mut buf, 0), Ok(()));
        let out: Scsireq = ioctl_arg(&buf);
        assert_eq!(out.retsts, SCCMD_OK);
        assert_eq!(out.datalen_used, 0);

        // REQUEST SENSE answers with sense data.
        screq.cmd[0] = 0x03;
        ioctl_ret(&mut buf, &screq);
        assert_eq!(scsi_do_ioctl(link, SCIOCCOMMAND, &mut buf, 0), Ok(()));
        let out: Scsireq = ioctl_arg(&buf);
        assert_eq!(out.retsts, SCCMD_SENSE);
        assert_eq!(usize::from(out.senselen_used), size_of::<ScsiSenseData>());
        assert_eq!(out.sense[0], SSD_ERRCODE_CURRENT);
        assert_eq!(out.sense[2], SKEY_ILLEGAL_REQUEST);

        // INQUIRY times out at this adapter.
        screq.cmd[0] = 0x12;
        ioctl_ret(&mut buf, &screq);
        assert_eq!(scsi_do_ioctl(link, SCIOCCOMMAND, &mut buf, 0), Ok(()));
        assert_eq!(ioctl_arg::<Scsireq>(&buf).retsts, SCCMD_UNKNOWN);

        // Bad lengths.
        screq.cmdlen = 17;
        ioctl_ret(&mut buf, &screq);
        assert_eq!(scsi_do_ioctl(link, SCIOCCOMMAND, &mut buf, 0), Err(EFAULT));
        screq.cmdlen = 6;
        screq.datalen = MAXPHYS as u64 + 1;
        ioctl_ret(&mut buf, &screq);
        assert_eq!(scsi_do_ioctl(link, SCIOCCOMMAND, &mut buf, 0), Err(EINVAL));
        teardown();
    }

    #[test]
    fn ata_commands_become_ata_passthrough() {
        let (_g, link) = setup();
        let mut buf = [0u8; size_of::<Atareq>()];
        let mut atareq = Atareq::default();
        atareq.command = 0xec; // IDENTIFY DEVICE, no data here
        ioctl_ret(&mut buf, &atareq);
        // ATA PASS-THROUGH(12) (0xa1) times out at the fake adapter.
        assert_eq!(scsi_do_ioctl(link, ATAIOCCOMMAND, &mut buf, FWRITE), Ok(()));
        assert_eq!(ioctl_arg::<Atareq>(&buf).retsts, ATACMD_ERROR);
        atareq.datalen = MAXPHYS as u64 + 1;
        ioctl_ret(&mut buf, &atareq);
        assert_eq!(
            scsi_do_ioctl(link, ATAIOCCOMMAND, &mut buf, FWRITE),
            Err(EINVAL)
        );
        teardown();
    }
}
/* </TESTS> */
