//! Host tests of `vioblk.rs`: the request slot free list, the SCSI command translation and
//! the replies the driver fakes.

use std::alloc::{Layout, alloc_zeroed};
use std::boxed::Box;
use std::vec::Vec;
use std::{assert, assert_eq};

use super::*;
use crate::scsi::scsiconf::{ScsiLink, ScsibusSoftc};

/// A leaked, zero-filled `T` (all-zero is valid for the softcs and the request slots).
fn zeroed<T>(n: usize) -> *mut T {
    let layout = Layout::array::<T>(n).unwrap();
    // SAFETY: a non-zero layout.
    let p = unsafe { alloc_zeroed(layout) };
    assert!(!p.is_null());
    p.cast()
}

/// A vioblk softc with `n` request slots on its free list (in `vioblk_alloc_reqs`' order),
/// `capacity` sectors, over a virtio softc with `features`.
fn softc(n: usize, capacity: u64, features: u64) -> &'static VioblkSoftc {
    // SAFETY: leaked zeroed blocks; all-zero is a valid softc.
    let sc: &'static VioblkSoftc = unsafe { &*zeroed::<VioblkSoftc>(1) };
    // SAFETY: as above.
    let vsc: &'static VirtioSoftc = unsafe { &*zeroed::<VirtioSoftc>(1) };
    vsc.sc_active_features.set(features);
    sc.sc_virtio.set(Some(vsc));
    sc.sc_capacity.set(capacity);
    sc.sc_freelist.init();
    mtx_init(&sc.sc_vr_mtx, IPL_BIO);
    sc.sc_reqs.set(zeroed::<VirtioBlkReq>(n));
    sc.sc_nreqs.set(n);
    for i in 0..n {
        let vr = sc.req(i);
        vr.vr_qe_index.set((i * ALLOC_SEGS) as i16);
        vr.vr_len.set(VIOBLK_DONE);
        // SAFETY: a fresh slot, leaked.
        unsafe { sc.sc_freelist.insert_head(vr) };
    }
    sc
}

fn cookie(sc: &VioblkSoftc) -> *mut c_void {
    ptr::from_ref(sc).cast_mut().cast()
}

/// The slot index of an opening.
fn index(sc: &VioblkSoftc, io: ScsiIo) -> usize {
    (io.as_ptr() as usize - sc.sc_reqs.get() as usize) / size_of::<VirtioBlkReq>()
}

#[test]
fn the_request_layout_is_the_c_one() {
    assert_eq!(offset_of!(VirtioBlkReq, vr_status), 16);
    assert_eq!(VR_DMA_END, 18);
    assert_eq!(SEG_MAX, MAXPHYS / PAGE_SIZE + 1);
    assert_eq!(ALLOC_SEGS, SEG_MAX + 2);
}

#[test]
fn req_get_and_put_cycle_the_free_list() {
    let sc = softc(3, 0, 0);
    let mut got = Vec::new();
    // SAFETY: the cookie is the live softc; every opening goes back once.
    unsafe {
        while let Some(io) = vioblk_req_get(cookie(sc)) {
            got.push(index(sc, io));
        }
    }
    // SLIST_INSERT_HEAD: the last slot made is handed out first.
    assert_eq!(got, [2, 1, 0]);
    assert!(sc.sc_freelist.is_empty());

    let io = NonNull::from(sc.req(1)).cast();
    // SAFETY: slot 1 was taken above and is on no list.
    unsafe { vioblk_req_put(cookie(sc), io) };
    // SAFETY: as above.
    let again = unsafe { vioblk_req_get(cookie(sc)) }.unwrap();
    assert_eq!(index(sc, again), 1);
    // SAFETY: as above.
    assert!(unsafe { vioblk_req_get(cookie(sc)) }.is_none());
}

#[test]
fn opcodes_map_to_virtio_requests() {
    let rd = VioblkCmd::Request {
        operation: VIRTIO_BLK_T_IN,
        isread: true,
    };
    let wr = VioblkCmd::Request {
        operation: VIRTIO_BLK_T_OUT,
        isread: false,
    };
    for op in [READ_COMMAND, READ_10, READ_12, READ_16] {
        assert_eq!(vioblk_scsi_op(op, false), rd);
    }
    for op in [WRITE_COMMAND, WRITE_10, WRITE_12, WRITE_16] {
        assert_eq!(vioblk_scsi_op(op, true), wr);
    }
    assert_eq!(
        vioblk_scsi_op(SYNCHRONIZE_CACHE, true),
        VioblkCmd::Request {
            operation: VIRTIO_BLK_T_FLUSH,
            isread: false
        }
    );
    assert_eq!(
        vioblk_scsi_op(SYNCHRONIZE_CACHE, false),
        VioblkCmd::Done(XS_NOERROR)
    );
    assert_eq!(vioblk_scsi_op(INQUIRY, false), VioblkCmd::Inquiry);
    assert_eq!(vioblk_scsi_op(READ_CAPACITY, false), VioblkCmd::Capacity);
    assert_eq!(
        vioblk_scsi_op(READ_CAPACITY_16, false),
        VioblkCmd::Capacity16
    );
    for op in [TEST_UNIT_READY, START_STOP, PREVENT_ALLOW] {
        assert_eq!(vioblk_scsi_op(op, false), VioblkCmd::Done(XS_NOERROR));
    }
    for op in [MODE_SENSE, MODE_SENSE_BIG, REPORT_LUNS] {
        assert_eq!(
            vioblk_scsi_op(op, false),
            VioblkCmd::Done(XS_DRIVER_STUFFUP)
        );
    }
    assert_eq!(vioblk_scsi_op(0x03, false), VioblkCmd::Unknown); // REQUEST SENSE
}

fn cdb(bytes: &[u8]) -> ScsiGeneric {
    ScsiGeneric::read_from(bytes)
}

#[test]
fn cdbs_decode_by_length() {
    // READ (6): 21-bit address, length 0 means 256.
    let c = cdb(&[READ_COMMAND, 0xff, 0x34, 0x56, 0, 0]);
    assert_eq!(vioblk_rw_decode(&c, 6), (0x1f_3456, 0x100));
    let c = cdb(&[WRITE_COMMAND, 0x01, 0x02, 0x03, 8, 0]);
    assert_eq!(vioblk_rw_decode(&c, 6), (0x01_0203, 8));
    // READ (10) and SYNCHRONIZE CACHE (10), same layout.
    let c = cdb(&[READ_10, 0, 0x12, 0x34, 0x56, 0x78, 0, 0x01, 0x00, 0]);
    assert_eq!(vioblk_rw_decode(&c, 10), (0x1234_5678, 0x100));
    let c = cdb(&[SYNCHRONIZE_CACHE, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(vioblk_rw_decode(&c, 10), (0, 0));
    // WRITE (12).
    let c = cdb(&[WRITE_12, 0, 0, 0, 0x10, 0, 0, 0x01, 0, 0, 0, 0]);
    assert_eq!(vioblk_rw_decode(&c, 12), (0x1000, 0x1_0000));
    // READ (16): 64-bit address.
    let c = cdb(&[
        READ_16, 0, 0x01, 0, 0, 0, 0, 0, 0, 0x02, 0, 0, 0, 0x80, 0, 0,
    ]);
    assert_eq!(vioblk_rw_decode(&c, 16), (0x0100_0000_0000_0002, 0x80));
    // Any other length leaves the C's zeroes.
    assert_eq!(vioblk_rw_decode(&c, 8), (0, 0));
}

#[test]
fn the_inquiry_reply_is_a_virtio_disk() {
    let inqd = vioblk_inquiry_data();
    let b = inqd.as_bytes();
    assert_eq!(
        &b[..8],
        &[T_DIRECT, 0, SCSI_REV_SPC3, 2, 31, 0, 0, SID_CmdQue]
    );
    assert_eq!(&b[8..16], b"VirtIO  ");
    assert_eq!(&b[16..32], b"Block Device    ");
    assert!(b[32..].iter().all(|&x| x == 0));
}

#[test]
fn capacity_replies_give_the_last_lba() {
    let rcd = vioblk_read_cap_data(2048);
    assert_eq!(rcd.as_bytes(), &[0, 0, 0x07, 0xff, 0, 0, 0x02, 0x00]);
    // Past 2^32 sectors, READ CAPACITY (10) says 0xffffffff.
    let rcd = vioblk_read_cap_data(0x1_0000_0002);
    assert_eq!(&rcd.addr, &[0xff; 4]);
    let rcd = vioblk_read_cap_data_16(0x1_0000_0002);
    assert_eq!(&rcd.addr, &[0, 0, 0, 0x01, 0, 0, 0, 0x01]);
    assert_eq!(&rcd.length, &[0, 0, 0x02, 0x00]);
    assert!(rcd.as_bytes()[12..].iter().all(|&x| x == 0));
}

fn nodone(_xs: &'static ScsiXfer) {}

/// A transfer on a link of a bus whose adapter softc is `sc`, with `cdb` and `data`.
fn xfer(sc: &'static VioblkSoftc, cdb: &[u8], data: &'static mut [u8]) -> &'static ScsiXfer {
    // SAFETY: a leaked zeroed block; all-zero is a valid bus softc.
    let bus: &'static ScsibusSoftc = unsafe { &*zeroed::<ScsibusSoftc>(1) };
    bus.sb_adapter_softc.set(cookie(sc));
    bus.sb_adapter.set(Some(&VIOBLK_SWITCH));
    let link: &'static ScsiLink = Box::leak(Box::new(ScsiLink::new()));
    link.bus.set(Some(bus));
    let xs: &'static ScsiXfer = Box::leak(Box::new(ScsiXfer::new()));
    xs.sc_link.set(Some(link));
    xs.cmd.set(ScsiGeneric::read_from(cdb));
    xs.cmdlen.set(cdb.len() as i32);
    // SAFETY: leaked, used by this transfer only.
    unsafe { xs.set_data(data.as_mut_ptr(), data.len() as i32) };
    xs.done.set(Some(nodone));
    xs.error.set(-1);
    xs
}

fn buf(n: usize) -> &'static mut [u8] {
    Box::leak(std::vec![0xaau8; n].into_boxed_slice())
}

#[test]
fn faked_commands_complete_at_once() {
    let sc = softc(1, 4096, 0);

    let data = buf(96);
    let p = data.as_ptr();
    let xs = xfer(sc, &[INQUIRY, 0, 0, 0, 96, 0], data);
    vioblk_scsi_cmd(xs);
    assert_eq!(xs.error.get(), XS_NOERROR);
    assert_eq!(xs.resid.get(), 0);
    // SAFETY: the leaked buffer, no longer written.
    let got = unsafe { core::slice::from_raw_parts(p, 96) };
    assert_eq!(got, vioblk_inquiry_data().as_bytes());

    // EVPD pages are refused.
    let xs = xfer(sc, &[INQUIRY, SI_EVPD, 0x80, 0, 96, 0], buf(96));
    vioblk_scsi_cmd(xs);
    assert_eq!(xs.error.get(), XS_DRIVER_STUFFUP);

    // READ CAPACITY copies at most datalen bytes.
    let data = buf(6);
    let p = data.as_ptr();
    let xs = xfer(sc, &[READ_CAPACITY, 0, 0, 0, 0, 0, 0, 0, 0, 0], data);
    vioblk_scsi_cmd(xs);
    assert_eq!(xs.error.get(), XS_NOERROR);
    // SAFETY: as above.
    let got = unsafe { core::slice::from_raw_parts(p, 6) };
    assert_eq!(got, &[0, 0, 0x0f, 0xff, 0, 0]);

    let data = buf(32);
    let p = data.as_ptr();
    let mut c16 = [0u8; 16];
    c16[0] = READ_CAPACITY_16;
    let xs = xfer(sc, &c16, data);
    vioblk_scsi_cmd(xs);
    assert_eq!(xs.error.get(), XS_NOERROR);
    // SAFETY: as above.
    let got = unsafe { core::slice::from_raw_parts(p, 32) };
    assert_eq!(got, vioblk_read_cap_data_16(4096).as_bytes());

    for c in [TEST_UNIT_READY, START_STOP, PREVENT_ALLOW] {
        let xs = xfer(sc, &[c, 0, 0, 0, 0, 0], buf(1));
        vioblk_scsi_cmd(xs);
        assert_eq!(xs.error.get(), XS_NOERROR);
    }
    for c in [MODE_SENSE, REPORT_LUNS, 0x03] {
        let xs = xfer(sc, &[c, 0, 0, 0, 0, 0], buf(1));
        vioblk_scsi_cmd(xs);
        assert_eq!(xs.error.get(), XS_DRIVER_STUFFUP);
    }
    // Without VIRTIO_BLK_F_FLUSH, SYNCHRONIZE CACHE succeeds at once.
    let xs = xfer(sc, &[SYNCHRONIZE_CACHE, 0, 0, 0, 0, 0, 0, 0, 0, 0], buf(1));
    vioblk_scsi_cmd(xs);
    assert_eq!(xs.error.get(), XS_NOERROR);
}
