//! Host tests for `sd.rs`: `viscpy`, the READ/WRITE CDBs, `sd_get_parms` (capacity, mode
//! pages and the geometry it settles on) and `sdstart`/`sd_buf_done` against a fake adapter
//! that completes every command at once, recording its CDB.

use std::alloc::{Layout, alloc_zeroed};
use std::boxed::Box;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::MutexGuard;
use std::vec::Vec;
use std::{assert, assert_eq, vec};

use super::*;
use crate::kern::subr_pool::{pool_destroy, pool_init};
use crate::machine::intr::IPL_BIO;
use crate::scsi::scsi_all::{SKEY_MEDIUM_ERROR, ScsiSenseData};
use crate::scsi::scsi_base::{
    SCSI_XFER_POOL, scsi_copy_internal_data, scsi_default_get, scsi_default_put, scsi_done,
    scsi_iopool_init,
};
use crate::scsi::scsiconf::{
    SCSI_RETRIES, ScsiAdapter, ScsiIopool, ScsibusSoftc, XS_DRIVER_STUFFUP,
};
use crate::scsi::sdvar::DiskParms;
use crate::sys::buf::B_DONE;
use crate::sys::disklabel::DTYPE_SCSI;

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
    /// The fake adapter's script.
    static SCRIPT: RefCell<VecDeque<Reply>> = const { RefCell::new(VecDeque::new()) };
    /// The CDB (`cmdlen` bytes) and data length of every command sent.
    static SENT: RefCell<Vec<(Vec<u8>, i32)>> = const { RefCell::new(Vec::new()) };
}

/// The fake adapter's `scsi_cmd`: completes `xs` at once with the next scripted reply (an
/// empty data reply when the script is out).
fn fake_cmd(xs: &'static ScsiXfer) {
    let reply = SCRIPT.with(|s| s.borrow_mut().pop_front());
    SENT.with(|s| {
        let cmd = xs.cmd.get();
        let len = xs.cmdlen.get() as usize;
        s.borrow_mut()
            .push((cmd.as_bytes()[..len].to_vec(), xs.datalen()));
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
    SCRIPT.with(|s| *s.borrow_mut() = replies.into());
    SENT.with(|s| s.borrow_mut().clear());
}

fn sent() -> Vec<(Vec<u8>, i32)> {
    SENT.with(|s| s.borrow().clone())
}

fn opcodes() -> Vec<u8> {
    sent().iter().map(|(cdb, _)| cdb[0]).collect()
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

/// A zeroed `T` (an `M_ZERO` allocation), leaked.
///
/// # Safety
///
/// All-zero must be a valid `T`.
unsafe fn leak_zeroed<T>() -> &'static T {
    // SAFETY: a fresh zeroed allocation of `T`'s layout; the caller vouches for zero.
    unsafe { &*alloc_zeroed(Layout::new::<T>()).cast::<T>() }
}

/// A link on the fake adapter, with its own default iopool, for a device of type `device`
/// and SCSI revision `version`.
fn test_link(device: u8, version: u8) -> &'static ScsiLink {
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
    // SAFETY: all-zero is a valid `ScsibusSoftc` (its `Softc` impl).
    let sb: &'static ScsibusSoftc = unsafe { leak_zeroed() };
    sb.sb_adapter.set(Some(&FAKE_ADAPTER));
    link.pool.set(Some(pool));
    link.bus.set(Some(sb));
    link.openings.set(1);
    let mut inq = link.inqdata.get();
    inq.device = device;
    inq.version = version;
    link.inqdata.set(inq);
    link
}

/// An sd softc driving `link`, as `sdattach` leaves it (without the commands).
fn test_sd(link: &'static ScsiLink) -> &'static SdSoftc {
    // SAFETY: all-zero is a valid `SdSoftc` (its `Softc` impl).
    let sc: &'static SdSoftc = unsafe { leak_zeroed() };
    sc.sc_link.set(Some(link));
    link.device_softc.set(Some(NonNull::from(&sc.sc_dev)));
    sc
}

/// READ CAPACITY (10) data: last block `last`, `secsize` bytes per block.
fn cap10(last: u32, secsize: u32) -> Reply {
    let mut d = vec![0u8; 8];
    _lto4b(last, &mut d[0..4]);
    _lto4b(secsize, &mut d[4..8]);
    Reply::Data(d)
}

/// READ CAPACITY (16) data.
fn cap16(last: u64, secsize: u32, tpe: bool) -> Reply {
    let mut d = vec![0u8; 32];
    _lto8b(last, &mut d[0..8]);
    _lto4b(secsize, &mut d[8..12]);
    if tpe {
        _lto2b(u32::from(READ_CAP_16_TPE), &mut d[14..16]);
    }
    Reply::Data(d)
}

/// A MODE SENSE (6) reply: the header (`dev_spec`), a direct-access block descriptor of
/// `blklen` bytes when it is not 0, and `page`.
fn mode6(dev_spec: u8, blklen: u32, page: &[u8]) -> Reply {
    let mut d = vec![0u8, 0, dev_spec, 0];
    if blklen != 0 {
        d[3] = 8;
        let mut desc = [0u8; 8];
        _lto3b(blklen, &mut desc[5..8]);
        d.extend_from_slice(&desc);
    }
    d.extend_from_slice(page);
    d[0] = (d.len() - 1) as u8;
    Reply::Data(d)
}

/// A rigid disk geometry page (4).
fn rigid_page(ncyl: u32, nheads: u8) -> Vec<u8> {
    let mut p = vec![0u8; size_of::<PageRigidGeometry>()];
    p[0] = PAGE_RIGID_GEOMETRY;
    p[1] = (p.len() - 2) as u8;
    _lto3b(ncyl, &mut p[2..5]);
    p[5] = nheads;
    p
}

#[test]
fn viscpy_skips_unprintables_and_stops() {
    let mut dst = [0xffu8; 9];
    viscpy(&mut dst, b"AB\x01C\x80DEFGHIJ", 8);
    assert_eq!(&dst, b"ABCDEFGH\0");

    // A NUL ends the copy; the field's end too.
    let mut dst = [0xffu8; 9];
    viscpy(&mut dst, b"QEMU\0XYZ", 8);
    assert_eq!(&dst[..5], b"QEMU\0");
    let mut dst = [0xffu8; 9];
    viscpy(&mut dst, b"\x7f\x7f", 8);
    assert_eq!(&dst[..3], b"\x7f\x7f\0");
}

#[test]
fn rw_cdbs() {
    let mut g = ScsiGeneric::zeroed();
    assert_eq!(sd_cmd_rw6(&mut g, true, 0x12_3456, 0x80), 6);
    assert_eq!(
        &g.as_bytes()[..6],
        &[READ_COMMAND, 0x12, 0x34, 0x56, 0x80, 0]
    );

    let mut g = ScsiGeneric::zeroed();
    assert_eq!(sd_cmd_rw10(&mut g, false, 0x0102_0304, 0x0506), 10);
    assert_eq!(&g.as_bytes()[..10], &[WRITE_10, 0, 1, 2, 3, 4, 0, 5, 6, 0]);

    let mut g = ScsiGeneric::zeroed();
    assert_eq!(sd_cmd_rw12(&mut g, true, 7, 0x0001_0000), 12);
    assert_eq!(
        &g.as_bytes()[..12],
        &[READ_12, 0, 0, 0, 0, 7, 0, 1, 0, 0, 0, 0]
    );

    let mut g = ScsiGeneric::zeroed();
    assert_eq!(sd_cmd_rw16(&mut g, false, 0x1_0000_0002, 3), 16);
    assert_eq!(
        &g.as_bytes()[..16],
        &[WRITE_16, 0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0]
    );
}

#[test]
fn get_parms_reads_capacity_and_rigid_geometry() {
    let _g = setup();
    let link = test_link(T_DIRECT, SCSI_REV_2);
    let sc = test_sd(link);

    script(vec![
        cap10(131_071, 512),
        mode6(SMH_DSP_WRITE_PROT, 0, &[]),
        mode6(0, 512, &rigid_page(128, 16)),
    ]);
    assert_eq!(sd_get_parms(sc, 0), Ok(()));
    assert_eq!(opcodes(), vec![0x25, 0x1a, 0x1a]);
    assert_eq!(
        sc.params.get(),
        DiskParms {
            heads: 16,
            cyls: 128,
            sectors: 64,
            secsize: 512,
            disksize: 131_072,
            unmap_sectors: 0,
            unmap_descs: 0,
        }
    );
    // Page 0's header said write protected.
    assert!(link_isset(link, SDEV_READONLY));
    assert!(!sc.isset(SDF_THIN));
    teardown();
}

#[test]
fn get_parms_falls_back_to_standard_geometry() {
    let _g = setup();
    let link = test_link(T_DIRECT, SCSI_REV_2);
    let sc = test_sd(link);

    // Every mode sense fails (no valid header): 255 heads, 63 sectors.
    script(vec![cap10(131_071, 0)]);
    assert_eq!(sd_get_parms(sc, 0), Ok(()));
    let dp = sc.params.get();
    assert_eq!(
        (dp.heads, dp.sectors, dp.cyls),
        (255, 63, 131_072 / (255 * 63))
    );
    // A secsize of 0 is DEV_BSIZE.
    assert_eq!(dp.secsize, 512);
    // Page 0, rigid and flexible geometry, each with MODE SENSE (6) and (10).
    assert_eq!(opcodes(), vec![0x25, 0x1a, 0x5a, 0x1a, 0x5a, 0x1a, 0x5a]);

    // A disk smaller than one such cylinder goes into one cylinder. (A fresh softc:
    // sd_get_parms starts from the geometry it found last time, as the C does.)
    let sc = test_sd(link);
    script(vec![cap10(999, 512)]);
    assert_eq!(sd_get_parms(sc, 0), Ok(()));
    let dp = sc.params.get();
    assert_eq!(
        (dp.heads, dp.cyls, dp.sectors, dp.disksize),
        (1, 1, 1000, 1000)
    );
    teardown();
}

#[test]
fn get_parms_large_disks_use_read_capacity_16() {
    let _g = setup();
    let link = test_link(T_DIRECT, SCSI_REV_2);
    let sc = test_sd(link);

    // READ CAPACITY (10) saturates: ask again with READ CAPACITY (16).
    script(vec![
        cap10(0xffff_ffff, 4096),
        cap16(0x2_0000_0000, 4096, false),
    ]);
    assert_eq!(sd_get_parms(sc, 0), Ok(()));
    assert_eq!(&opcodes()[..2], &[0x25, 0x9e]);
    let dp = sc.params.get();
    assert_eq!(dp.disksize, 0x2_0000_0001);
    assert_eq!(dp.secsize, 4096);
    assert_eq!((dp.heads, dp.sectors), (511, 255));
    assert_eq!(dp.cyls, (0x2_0000_0001u64 / (511 * 255)) as u32);

    // A post-SPC2 device starts with READ CAPACITY (16); TPE marks it thin, but without
    // the VPD pages (none answer) thin provisioning is dropped.
    let link = test_link(T_DIRECT, 6);
    let sc = test_sd(link);
    script(vec![cap16(2047, 512, true)]);
    assert_eq!(sd_get_parms(sc, 0), Ok(()));
    assert_eq!(opcodes()[..2], [0x9e, 0x12]);
    assert!(!sc.isset(SDF_THIN));
    assert_eq!(sc.params.get().disksize, 2048);
    teardown();
}

#[test]
fn get_parms_rejects_bad_capacity() {
    let _g = setup();
    let link = test_link(T_DIRECT, SCSI_REV_2);
    let sc = test_sd(link);

    // A sector size that is not a power of two between 512 and 64k.
    script(vec![cap10(1000, 520)]);
    assert_eq!(sd_get_parms(sc, 0), Err(EIO));
    // No capacity at all.
    script(vec![cap10(0, 512)]);
    assert_eq!(sd_get_parms(sc, 0), Err(EIO));
    teardown();
}

/// A softc ready for `sdstart`: media loaded, a fifo queue and a label with partition `a`
/// at sector 64.
fn started_sd(version: u8, disksize: u64) -> &'static SdSoftc {
    let link = test_link(T_DIRECT, version);
    let sc = test_sd(link);
    link_set(link, SDEV_MEDIA_LOADED);
    bufq_init(&sc.sc_bufq, BUFQ_FIFO).expect("bufq_init");
    let mut lp = Disklabel::zeroed();
    lp.d_secsize = 512;
    lp.d_type = DTYPE_SCSI;
    lp.d_partitions[0].p_offset = 64;
    let lp: &'static mut Disklabel = Box::leak(Box::new(lp));
    sc.sc_dk.dk_label.set(Some(NonNull::from(lp)));
    let mut dp = sc.params.get();
    dp.disksize = disksize;
    dp.secsize = 512;
    sc.params.set(dp);
    sc
}

/// A buffer of `bcount` bytes at block `blkno` of partition `a`, backed by real memory.
fn test_buf(flags: i64, blkno: Daddr, bcount: usize) -> &'static Buf {
    let bp: &'static Buf = Box::leak(Box::new(Buf::new()));
    let data: &'static mut [u8] = Box::leak(vec![0u8; bcount].into_boxed_slice());
    bp.b_flags.set(flags);
    bp.b_blkno.set(blkno);
    bp.b_bcount.set(bcount as i64);
    bp.b_data.set(data.as_mut_ptr());
    bp
}

/// Queues `bp` and runs `sdstart` once, as the transfer handler would.
fn start(sc: &'static SdSoftc, bp: &'static Buf) {
    bufq_queue(&sc.sc_bufq, bp);
    let xs = scsi_xs_get(sc.link(), 0).expect("scsi_xs_get");
    sdstart(xs);
}

#[test]
fn sdstart_picks_the_cdb_size() {
    let _g = setup();

    // A SCSI-1 device, small transfer: READ (6), at the partition's offset.
    let sc = started_sd(1, 131_072);
    script(vec![]);
    let bp = test_buf(B_READ, 8, 1024);
    start(sc, bp);
    assert_eq!(sent()[0].0, vec![READ_COMMAND, 0, 0, 64 + 8, 2, 0]);
    assert_eq!(sent()[0].1, 1024);
    assert!(bp.isset(B_DONE) && !bp.isset(B_ERROR));
    assert!(!sc.isset(SDF_DIRTY));

    // SCSI-2: WRITE (10), and the disk is dirty.
    let sc = started_sd(SCSI_REV_2, 131_072);
    script(vec![]);
    start(sc, test_buf(B_WRITE, 0, 512));
    assert_eq!(sent()[0].0, vec![WRITE_10, 0, 0, 0, 0, 64, 0, 0, 1, 0]);
    assert!(sc.isset(SDF_DIRTY));

    // More than 65535 sectors: READ (12).
    let sc = started_sd(SCSI_REV_2, 1 << 20);
    script(vec![]);
    start(sc, test_buf(B_READ, 0, 65536 * 512));
    assert_eq!(sent()[0].0[0], READ_12);
    assert_eq!(&sent()[0].0[6..10], &[0, 1, 0, 0]);

    // A disk past 2^32 sectors: READ (16).
    let sc = started_sd(SCSI_REV_2, 1 << 33);
    script(vec![]);
    start(sc, test_buf(B_READ, 0, 512));
    assert_eq!(sent()[0].0.len(), 16);
    assert_eq!(sent()[0].0[0], READ_16);
    teardown();
}

#[test]
fn sd_buf_done_residual_and_errors() {
    let _g = setup();
    let sc = started_sd(SCSI_REV_2, 131_072);

    // A short read: no error, the residual is what did not arrive.
    script(vec![Reply::Data(vec![0x5a; 512])]);
    let bp = test_buf(B_READ, 0, 1024);
    start(sc, bp);
    assert!(bp.isset(B_DONE) && !bp.isset(B_ERROR));
    assert_eq!(bp.b_resid.get(), 512);
    assert_eq!(bp.b_error.get(), None);
    // SAFETY: the test's buffer, done.
    assert_eq!(unsafe { *bp.b_data.get() }, 0x5a);

    // Timeouts are retried SCSI_RETRIES times, then fail with EIO and nothing done.
    let replies = (0..=SCSI_RETRIES)
        .map(|_| Reply::Error(XS_TIMEOUT))
        .collect();
    script(replies);
    let bp = test_buf(B_READ, 0, 512);
    start(sc, bp);
    assert_eq!(sent().len(), SCSI_RETRIES as usize + 1);
    assert!(bp.isset(B_ERROR));
    assert_eq!(bp.b_error.get(), Some(EIO));
    assert_eq!(bp.b_resid.get(), 512);

    // A driver error is not retried.
    script(vec![Reply::Error(XS_DRIVER_STUFFUP)]);
    let bp = test_buf(B_WRITE, 0, 512);
    start(sc, bp);
    assert_eq!(sent().len(), 1);
    assert_eq!(bp.b_error.get(), Some(EIO));

    // A medium error is the sense's errno, not retried.
    let mut sense = ScsiSenseData::new();
    sense.error_code = SSD_ERRCODE_CURRENT;
    sense.flags = SKEY_MEDIUM_ERROR;
    sense.extra_len = 10;
    script(vec![Reply::Sense(sense)]);
    let bp = test_buf(B_READ, 0, 512);
    start(sc, bp);
    assert_eq!(sent().len(), 1);
    assert!(bp.isset(B_ERROR));
    assert_eq!(bp.b_error.get(), Some(EIO));
    assert_eq!(bp.b_resid.get(), 512);
    teardown();
}

#[test]
fn ioctl_argument_helpers() {
    let mut raw = [0u8; 8];
    dk_cache_store(
        &DkCache {
            wrcache: 1,
            rdcache: 0,
        },
        &mut raw,
    );
    let dkc = dk_cache_load(&raw);
    assert_eq!((dkc.wrcache, dkc.rdcache), (1, 0));
    assert_eq!(ioctl_int(&7i32.to_ne_bytes()), 7);
    assert_eq!(mtop_op(&MTOFFL.to_ne_bytes()), MTOFFL);

    let mut di = dk_inquiry_zeroed();
    di.vendor[0] = b'V';
    di.serial[0] = b'S';
    let mut out = vec![0u8; size_of::<DkInquiry>()];
    dk_inquiry_store(&di, &mut out);
    assert_eq!(out[0], b'V');
    assert_eq!(out[64 + 128 + 64], b'S');
}
