//! Host tests for `cd.rs`: the READ/WRITE CDBs, the MSF arithmetic of `cd_play_tracks`,
//! `cd_size`/`cd_get_parms` (capacity, the defaults and the lying drives), the table of
//! contents and the play, mode page and DVD commands against a fake adapter that completes
//! every command at once, recording its CDB.

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
use crate::scsi::scsi_all::{MODE_SELECT, MODE_SENSE, SKEY_UNIT_ATTENTION, ScsiSenseData};
use crate::scsi::scsi_base::{
    SCSI_XFER_POOL, scsi_copy_internal_data, scsi_default_get, scsi_default_put, scsi_done,
    scsi_iopool_init,
};
use crate::scsi::scsiconf::{ScsiAdapter, ScsiIopool, ScsibusSoftc, XS_DRIVER_STUFFUP};
use crate::sys::cdio::{CD_LU_LOAD, DvdAuthinfo, DvdStruct};

/// What the fake adapter does with the next command.
enum Reply {
    /// Copies the bytes in (`scsi_copy_internal_data`; nothing for an empty reply).
    Data(Vec<u8>),
    /// Ends the command with this `XS_*` error.
    Error(i32),
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

/// A link on the fake adapter, with its own default iopool, for a CD-ROM of SCSI revision
/// `version`.
fn test_link(version: u8) -> &'static ScsiLink {
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
    inq.device = T_CDROM;
    inq.version = version;
    link.inqdata.set(inq);
    link
}

/// A cd softc driving `link`, as `cdattach` leaves it (without the commands).
fn test_cd(link: &'static ScsiLink) -> &'static CdSoftc {
    // SAFETY: all-zero is a valid `CdSoftc` (its `Softc` impl).
    let sc: &'static CdSoftc = unsafe { leak_zeroed() };
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
fn cap16(last: u64, secsize: u32) -> Reply {
    let mut d = vec![0u8; 32];
    crate::scsi::scsiconf::_lto8b(last, &mut d[0..8]);
    _lto4b(secsize, &mut d[8..12]);
    Reply::Data(d)
}

/// The bytes of a table of contents with tracks `first..=last` plus the lead-out, each
/// entry's address the `(m, s, f)` of `msf[i]` (in the layout the device writes: header
/// length in big-endian).
fn toc_bytes(first: u8, last: u8, msf: &[(u8, u8, u8)]) -> Vec<u8> {
    let n = usize::from(last - first) + 2;
    let mut d = vec![0u8; size_of::<IocTocHeader>() + n * size_of::<CdTocEntry>()];
    let len = (d.len() - 2) as u16;
    d[0..2].copy_from_slice(&len.to_be_bytes());
    d[2] = first;
    d[3] = last;
    for (i, &(m, s, f)) in msf.iter().enumerate().take(n) {
        let off = size_of::<IocTocHeader>() + i * size_of::<CdTocEntry>();
        d[off + 1] = 0x10; // control 0 (audio), addr_type 1
        d[off + 2] = first + i as u8;
        d[off + 4..off + 8].copy_from_slice(&[0, m, s, f]);
    }
    d
}

#[test]
fn rw_cdbs() {
    let mut g = ScsiGeneric::zeroed();
    assert_eq!(cd_cmd_rw6(&mut g, true, 0x12_3456, 0x80), 6);
    assert_eq!(
        &g.as_bytes()[..6],
        &[READ_COMMAND, 0x12, 0x34, 0x56, 0x80, 0]
    );

    let mut g = ScsiGeneric::zeroed();
    assert_eq!(cd_cmd_rw10(&mut g, false, 0x0102_0304, 0x0506), 10);
    assert_eq!(&g.as_bytes()[..10], &[WRITE_10, 0, 1, 2, 3, 4, 0, 5, 6, 0]);

    let mut g = ScsiGeneric::zeroed();
    assert_eq!(cd_cmd_rw12(&mut g, true, 7, 0x0001_0000), 12);
    assert_eq!(
        &g.as_bytes()[..12],
        &[READ_12, 0, 0, 0, 0, 7, 0, 1, 0, 0, 0, 0]
    );
}

#[test]
fn msf_prev_borrows_across_frames_seconds_and_minutes() {
    assert_eq!(cd_msf_prev(1, 2, 3), Ok((1, 2, 2)));
    assert_eq!(cd_msf_prev(1, 2, 0), Ok((1, 1, 74)));
    assert_eq!(cd_msf_prev(1, 0, 0), Ok((0, 59, 74)));
    assert_eq!(cd_msf_prev(0, 0, 0), Err(EINVAL));
}

#[test]
fn toc_accessors_decode_the_device_layout() {
    let mut toc = CdToc::new();
    toc.bytes[..12].copy_from_slice(&toc_bytes(1, 1, &[(0, 2, 0), (3, 4, 5)])[..12]);
    let th = toc.header();
    assert_eq!((th.starting_track, th.ending_track), (1, 1));
    assert_eq!(u16::from_be(th.len), 18);
    let e = toc.entry(0).unwrap();
    assert_eq!((e.control(), e.addr_type(), e.track), (0, 1, 1));
    assert_eq!(e.addr.second(), 2);
    assert!(toc.entry(MAXTRACK).is_some() && toc.entry(MAXTRACK + 1).is_none());

    let mut e = CdTocEntry::default();
    e.set_addr_type(CD_LBA_FORMAT);
    e.addr.set_lba(0x1234);
    toc.set_entry(3, &e);
    assert_eq!(toc.entry(3), Some(e));
    toc.set_entry(MAXTRACK + 1, &e);
    assert_eq!(toc.entries().len(), (MAXTRACK + 1) * 8);
}

#[test]
fn audio_page_has_the_ports_after_eight_bytes() {
    let mut page = [0u8; 16];
    let p: &mut CdAudioPage = wire_mut(&mut page);
    p.port[LEFT_PORT].channels = LEFT_CHANNEL;
    p.port[RIGHT_PORT].volume = 0x7f;
    assert_eq!(page[8], 1);
    assert_eq!(page[11], 0x7f);
}

#[test]
fn cd_size_reads_capacity_10_then_16_for_spc_devices() {
    let _g = setup();

    // SCSI-2 device: READ CAPACITY (10) and done.
    let link = test_link(SCSI_REV_2);
    script(vec![cap10(0x0002_bf1f, 2048)]);
    let mut secsize = 0;
    assert_eq!(cd_size(link, 0, Some(&mut secsize)), 0x2_bf20);
    assert_eq!(secsize, 2048);
    assert_eq!(opcodes(), vec![crate::scsi::scsi_disk::READ_CAPACITY]);

    // SPC device: then READ CAPACITY (16), whose values win.
    let link = test_link(SCSI_REV_SPC);
    script(vec![cap10(0x0002_bf1f, 2048), cap16(0x0002_bf1f, 2048)]);
    let mut secsize = 0;
    assert_eq!(cd_size(link, 0, Some(&mut secsize)), 0x2_bf20);
    assert_eq!(
        opcodes(),
        vec![
            crate::scsi::scsi_disk::READ_CAPACITY,
            crate::scsi::scsi_disk::READ_CAPACITY_16
        ]
    );

    // SPC device whose READ CAPACITY (16) fails: the (10) values stand.
    script(vec![cap10(99, 512), Reply::Error(XS_DRIVER_STUFFUP)]);
    assert_eq!(cd_size(link, 0, Some(&mut secsize)), 100);
    assert_eq!(secsize, 512);

    // A device that cannot say: 0.
    script(vec![Reply::Error(XS_DRIVER_STUFFUP)]);
    assert_eq!(cd_size(link, 0, None), 0);

    // More than 2^32 - 1 sectors and no (16): nothing.
    script(vec![
        cap10(0xffff_ffff, 2048),
        Reply::Error(XS_DRIVER_STUFFUP),
    ]);
    assert_eq!(cd_size(link, 0, Some(&mut secsize)), 0);
    assert_eq!(secsize, 0);
    teardown();
}

#[test]
fn get_parms_has_defaults_for_lying_drives() {
    let _g = setup();
    let link = test_link(SCSI_REV_2);
    let sc = test_cd(link);

    script(vec![cap10(0x0002_bf1f, 2048)]);
    assert_eq!(cd_get_parms(sc, 0), Ok(()));
    assert_eq!(
        sc.params.get(),
        CdParms {
            secsize: 2048,
            disksize: 0x2_bf20
        }
    );

    // A sector size that is not a multiple of 512 is a lie; so is a tiny disc.
    script(vec![cap10(5, 300)]);
    assert_eq!(cd_get_parms(sc, 0), Ok(()));
    assert_eq!(
        sc.params.get(),
        CdParms {
            secsize: 2048,
            disksize: 400000
        }
    );

    // A failed READ CAPACITY leaves the defaults.
    script(vec![Reply::Error(XS_DRIVER_STUFFUP)]);
    assert_eq!(cd_get_parms(sc, 0), Ok(()));
    assert_eq!(
        sc.params.get(),
        CdParms {
            secsize: 2048,
            disksize: 400000
        }
    );

    // ADEV_NOCAPACITY: no command at all.
    link.quirks.set(ADEV_NOCAPACITY);
    script(vec![]);
    assert_eq!(cd_get_parms(sc, 0), Ok(()));
    assert!(sent().is_empty());
    teardown();
}

#[test]
fn load_toc_asks_for_the_header_then_every_entry() {
    let _g = setup();
    let link = test_link(SCSI_REV_2);
    let sc = test_cd(link);
    let full = toc_bytes(1, 2, &[(0, 2, 0), (3, 0, 0), (7, 30, 10)]);

    script(vec![
        Reply::Data(full[..4].to_vec()),
        Reply::Data(full.clone()),
    ]);
    let mut toc = CdToc::new();
    assert_eq!(cd_load_toc(sc, &mut toc, CD_MSF_FORMAT), Ok(()));
    let cmds = sent();
    assert_eq!(cmds.len(), 2);
    // READ TOC: opcode, MSF bit (second command only), from_track, data_len.
    assert_eq!(cmds[0].0[0], READ_TOC);
    assert_eq!(cmds[0].0[1], 0);
    assert_eq!(cmds[0].1, 4);
    assert_eq!(cmds[1].0[1], CD_MSF);
    assert_eq!(cmds[1].1, 4 + 3 * 8);
    assert_eq!(&cmds[1].0[7..9], &[0, 28]);
    assert_eq!(toc.entry(2).unwrap().addr.minute(), 7);

    // An ending track before the starting one: EIO after the first command.
    let mut bad = toc_bytes(1, 1, &[]);
    bad[2] = 5;
    bad[3] = 4;
    script(vec![Reply::Data(bad[..4].to_vec())]);
    assert_eq!(cd_load_toc(sc, &mut toc, CD_LBA_FORMAT), Err(EIO));
    assert_eq!(sent().len(), 1);
    teardown();
}

#[test]
fn play_tracks_ends_one_frame_before_the_next_track() {
    let _g = setup();
    let link = test_link(SCSI_REV_2);
    let sc = test_cd(link);
    let full = toc_bytes(1, 2, &[(0, 2, 0), (3, 0, 0), (7, 30, 10)]);

    // Tracks 1 to 1: from entry 0 to just before entry 1.
    script(vec![
        Reply::Data(full[..4].to_vec()),
        Reply::Data(full.clone()),
    ]);
    assert_eq!(cd_play_tracks(sc, 1, 0, 1, 0), Ok(()));
    let cmds = sent();
    let play = &cmds[2].0;
    assert_eq!(play[0], PLAY_MSF);
    assert_eq!(&play[3..9], &[0, 2, 0, 2, 59, 74]);

    // No end track, a backwards range, and a start before the first track.
    assert_eq!(cd_play_tracks(sc, 1, 0, 0, 0), Err(EIO));
    assert_eq!(cd_play_tracks(sc, 3, 0, 2, 0), Err(EINVAL));
    script(vec![Reply::Data(full[..4].to_vec()), Reply::Data(full)]);
    assert_eq!(cd_play_tracks(sc, 0, 0, 2, 0), Err(EINVAL));
    teardown();
}

#[test]
fn simple_commands_have_their_cdbs() {
    let _g = setup();
    let link = test_link(SCSI_REV_2);
    let sc = test_cd(link);

    script(vec![]);
    assert_eq!(cd_play(sc, 0x0102_0304, 0x0506), Ok(()));
    assert_eq!(cd_pause(sc, 1), Ok(()));
    assert_eq!(cd_load_unload(sc, i32::from(CD_LU_LOAD), 3), Ok(()));
    assert_eq!(cd_play_msf(sc, 1, 2, 3, 4, 5, 6), Ok(()));
    assert_eq!(cd_reset(sc), Ok(()));
    let cmds = sent();
    assert_eq!(&cmds[0].0[..10], &[PLAY, 0, 1, 2, 3, 4, 0, 5, 6, 0]);
    assert_eq!(cmds[1].0[0], PAUSE);
    assert_eq!(cmds[1].0[8], 1);
    assert_eq!(cmds[2].0[0], LOAD_UNLOAD);
    assert_eq!((cmds[2].0[4], cmds[2].0[8]), (CD_LU_LOAD, 3));
    assert_eq!(&cmds[3].0[..10], &[PLAY_MSF, 0, 0, 1, 2, 3, 4, 5, 6, 0]);
    teardown();
}

#[test]
fn setchan_reads_the_audio_page_and_selects_it_back() {
    let _g = setup();
    let link = test_link(SCSI_REV_2);
    let sc = test_cd(link);

    // MODE SENSE (6) reply: header (no block descriptor) and the audio page.
    let mut page = vec![0u8; size_of::<CdAudioPage>()];
    page[0] = AUDIO_PAGE;
    page[1] = 14;
    let mut d = vec![0u8, 0, 0, 0];
    d.extend_from_slice(&page);
    d[0] = (d.len() - 1) as u8;
    script(vec![Reply::Data(d)]);
    assert_eq!(
        cd_setchan(
            sc,
            BOTH_CHANNEL,
            BOTH_CHANNEL,
            MUTE_CHANNEL,
            MUTE_CHANNEL,
            0
        ),
        Ok(())
    );
    assert_eq!(opcodes(), vec![MODE_SENSE, MODE_SELECT]);

    // A device without the page: EIO, no select.
    script(vec![Reply::Data(vec![3, 0, 0, 0])]);
    assert_eq!(cd_setchan(sc, 1, 2, 0, 0, 0), Err(EIO));
    assert_eq!(opcodes(), vec![MODE_SENSE]);

    // cd_getvol answers success even then.
    let mut v = IocVol::default();
    script(vec![Reply::Error(XS_DRIVER_STUFFUP)]);
    assert_eq!(cd_getvol(sc, &mut v, 0), Ok(()));
    teardown();
}

#[test]
fn dvd_requests_check_their_type_and_parse_the_replies() {
    let _g = setup();
    let link = test_link(SCSI_REV_2);
    let sc = test_cd(link);

    // Unknown types: EINVAL for a structure, ENOTTY for an authentication step, no command.
    script(vec![]);
    let mut s = DvdStruct::zeroed();
    s.physical().r#type = 0x40;
    assert_eq!(dvd_read_struct(sc, &mut s), Err(EINVAL));
    let mut a = DvdAuthinfo::zeroed();
    a.set_type(0x40);
    assert_eq!(dvd_auth(sc, &mut a), Err(ENOTTY));
    assert!(sent().is_empty());

    // REPORT KEY, AGID: the AGID is in the top two bits of byte 7.
    let mut reply = vec![0u8; 8];
    reply[7] = 2 << 6;
    script(vec![Reply::Data(reply)]);
    let mut a = DvdAuthinfo::zeroed();
    a.set_type(DVD_LU_SEND_AGID);
    assert_eq!(dvd_auth(sc, &mut a), Ok(()));
    assert_eq!(a.lsa().agid, 2);
    let cmds = sent();
    assert_eq!(cmds[0].0[0], GPCMD_REPORT_KEY);
    // cmd->bytes[8] is the length (8), bytes[9] the AGID/format byte (0).
    assert_eq!((cmds[0].0[9], cmds[0].0[10], cmds[0].1), (8, 0, 8));

    // The challenge goes out with its length bytes set; success moves the state on.
    script(vec![]);
    let mut a = DvdAuthinfo::zeroed();
    a.set_type(DVD_HOST_SEND_CHALLENGE);
    a.hsc().agid = 1;
    a.hsc().chal = [9; DVD_CHALLENGE_SIZE];
    assert_eq!(dvd_auth(sc, &mut a), Ok(()));
    assert_eq!(a.r#type(), DVD_LU_SEND_KEY1);
    assert_eq!(sent()[0].0[0], GPCMD_SEND_KEY);

    // A failed KEY2 exchange ends in DVD_AUTH_FAILURE.
    script(vec![Reply::Error(XS_DRIVER_STUFFUP)]);
    let mut a = DvdAuthinfo::zeroed();
    a.set_type(DVD_HOST_SEND_KEY2);
    assert!(dvd_auth(sc, &mut a).is_err());
    assert_eq!(a.r#type(), DVD_AUTH_FAILURE);

    // READ DVD STRUCTURE, physical: the layers' fields come out of the 20-byte records.
    let mut reply = vec![0u8; DVD_READ_PHYSICAL_BUFSIZE];
    reply[4] = 0x21; // book type 2, version 1
    reply[4 + 4..4 + 8].copy_from_slice(&0x0003_0000u32.to_be_bytes());
    script(vec![Reply::Data(reply)]);
    let mut s = DvdStruct::zeroed();
    s.physical().r#type = DVD_STRUCT_PHYSICAL;
    s.physical().layer_num = 0;
    assert_eq!(dvd_read_struct(sc, &mut s), Ok(()));
    assert_eq!(s.physical().layer[0].book_type, 2);
    assert_eq!(s.physical().layer[0].book_version, 1);
    assert_eq!(s.physical().layer[0].start_sector, 0x3_0000);
    assert_eq!(sent()[0].0[6], DVD_STRUCT_PHYSICAL);

    // BCA: a length outside 12..=188 is EIO.
    let mut reply = vec![0u8; DVD_READ_BCA_BUFLEN];
    reply[1] = 4;
    script(vec![Reply::Data(reply)]);
    let mut s = DvdStruct::zeroed();
    s.bca().r#type = DVD_STRUCT_BCA;
    assert_eq!(dvd_read_struct(sc, &mut s), Err(EIO));
    teardown();
}

#[test]
fn interpret_sense_counts_becoming_ready_as_no_retry() {
    let _g = setup();
    let link = test_link(SCSI_REV_2);
    link.flags.set(link.flags.get() | SDEV_OPEN);

    let xs: &'static ScsiXfer = Box::leak(Box::new(ScsiXfer::new()));
    xs.sc_link.set(Some(link));
    xs.flags.set(crate::scsi::scsiconf::SCSI_NOSLEEP);
    xs.retries.set(2);
    xs.sense.set(ScsiSenseData {
        error_code: SSD_ERRCODE_CURRENT,
        flags: SKEY_NOT_READY,
        extra_len: 10,
        add_sense_code: 0x04,
        add_sense_code_qual: 0x01,
        ..ScsiSenseData::new()
    });
    // `retries` is incremented (the caller's decrement cancels it) and the command retried.
    assert_eq!(cd_interpret_sense(xs), Err(ERESTART));
    assert_eq!(xs.retries.get(), 3);

    // SCSI_IGNORE_NOT_READY: success.
    xs.flags.set(SCSI_IGNORE_NOT_READY);
    assert_eq!(cd_interpret_sense(xs), Ok(()));

    // Anything else is the generic code's (a unit attention is retried).
    xs.sense.set(ScsiSenseData {
        error_code: SSD_ERRCODE_CURRENT,
        flags: SKEY_UNIT_ATTENTION,
        ..ScsiSenseData::new()
    });
    xs.flags.set(0);
    assert_eq!(cd_interpret_sense(xs), scsi_interpret_sense(xs));
    teardown();
}
