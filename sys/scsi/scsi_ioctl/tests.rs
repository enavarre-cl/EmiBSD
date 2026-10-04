//! Host tests for `scsi_ioctl.rs`: the read-safe table, the permission checks, and the
//! commands without user data against an adapter that completes at once.

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
