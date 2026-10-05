//! Host tests of `vioscsi.rs`: the request slot layout and free list, the LUN encoding the
//! device demands and the translation of the response header into the transfer.

use std::alloc::{Layout, alloc_zeroed};
use std::vec::Vec;
use std::{assert, assert_eq};

use super::*;
use crate::scsi::scsi_all::ScsiSenseData;
use crate::scsi::scsiconf::XS_SENSE;

/// A leaked, zero-filled `T` (all-zero is valid for the softcs and the request slots).
fn zeroed<T>(n: usize) -> *mut T {
    let layout = Layout::array::<T>(n).unwrap();
    // SAFETY: a non-zero layout.
    let p = unsafe { alloc_zeroed(layout) };
    assert!(!p.is_null());
    p.cast()
}

/// A vioscsi softc with `n` request slots on its free list (in `vioscsi_alloc_reqs`' order).
fn softc(n: usize) -> &'static VioscsiSoftc {
    // SAFETY: leaked zeroed block; all-zero is a valid softc.
    let sc: &'static VioscsiSoftc = unsafe { &*zeroed::<VioscsiSoftc>(1) };
    sc.sc_freelist.init();
    mtx_init(&sc.sc_vr_mtx, IPL_BIO);
    sc.sc_reqs.set(zeroed::<VioscsiReq>(n));
    sc.sc_nreqs.set(n);
    for i in 0..n {
        let vr = sc.req(i);
        vr.vr_qe_index.set((i * ALLOC_SEGS) as i32);
        // SAFETY: a fresh slot, leaked.
        unsafe { sc.sc_freelist.insert_head(vr) };
    }
    sc
}

fn cookie(sc: &VioscsiSoftc) -> *mut c_void {
    ptr::from_ref(sc).cast_mut().cast()
}

/// The slot index of an opening.
fn index(sc: &VioscsiSoftc, io: ScsiIo) -> usize {
    (io.as_ptr() as usize - sc.sc_reqs.get() as usize) / size_of::<VioscsiReq>()
}

#[test]
fn the_request_layout_is_the_c_one() {
    // 51 bytes of request header, 108 of response header, then the pointers at 8 bytes.
    assert_eq!(offset_of!(VioscsiReq, vr_req), 0);
    assert_eq!(offset_of!(VioscsiReq, vr_res), 51);
    assert_eq!(VR_DMA_END, 160);
    assert_eq!(SEG_MAX, MAXPHYS / PAGE_SIZE + 1);
    assert_eq!(ALLOC_SEGS, SEG_MAX + 2);
}

#[test]
fn req_get_and_put_cycle_the_free_list() {
    let sc = softc(3);
    let mut got = Vec::new();
    // SAFETY: the cookie is the live softc; every opening goes back once.
    unsafe {
        while let Some(io) = vioscsi_req_get(cookie(sc)) {
            got.push(index(sc, io));
        }
    }
    // SLIST_INSERT_HEAD: the last slot made is handed out first.
    assert_eq!(got, [2, 1, 0]);
    assert!(sc.sc_freelist.is_empty());

    let io = NonNull::from(sc.req(1)).cast();
    // SAFETY: slot 1 was taken above and is on no list.
    unsafe { vioscsi_req_put(cookie(sc), io) };
    // SAFETY: as above.
    let again = unsafe { vioscsi_req_get(cookie(sc)) }.unwrap();
    assert_eq!(index(sc, again), 1);
    // SAFETY: as above.
    assert!(unsafe { vioscsi_req_get(cookie(sc)) }.is_none());
}

#[test]
fn the_lun_is_a_single_level_structure() {
    assert_eq!(vioscsi_lun(0, 0), Some([1, 0, 0x40, 0, 0, 0, 0, 0]));
    assert_eq!(vioscsi_lun(3, 0x0102), Some([1, 3, 0x41, 0x02, 0, 0, 0, 0]));
    assert_eq!(
        vioscsi_lun(255, 16383),
        Some([1, 255, 0x7f, 0xff, 0, 0, 0, 0])
    );
    assert_eq!(vioscsi_lun(256, 0), None);
    assert_eq!(vioscsi_lun(0, 16384), None);
}

#[test]
fn an_ok_response_carries_status_residual_and_sense() {
    let xs = ScsiXfer::new();
    let mut res = VirtioScsiResHdr::new();
    res.status = 2; // CHECK CONDITION
    res.residual = 512;
    res.sense_len = 18;
    res.sense[0] = 0x70;
    res.sense[2] = 0x05;
    res.sense[12] = 0x24;
    vioscsi_xs_result(&xs, &res);
    assert_eq!(xs.error.get(), XS_SENSE);
    assert_eq!(xs.status.get(), 2);
    assert_eq!(xs.resid.get(), 512);
    let sense = xs.sense.get();
    assert_eq!(
        (sense.error_code, sense.flags, sense.add_sense_code),
        (0x70, 0x05, 0x24)
    );

    // No sense data: no error; a longer sense length than the structure is clamped.
    let xs = ScsiXfer::new();
    let mut res = VirtioScsiResHdr::new();
    res.status = 0;
    vioscsi_xs_result(&xs, &res);
    assert_eq!(xs.error.get(), XS_NOERROR);
    assert_eq!(xs.sense.get(), ScsiSenseData::new());
    res.sense_len = 96;
    res.sense = [0xaa; 96];
    vioscsi_xs_result(&xs, &res);
    assert_eq!(xs.error.get(), XS_SENSE);
    assert_eq!(xs.sense.get().sense_key_spec_3, 0xaa);
}

#[test]
fn a_failed_response_is_a_driver_stuffup_with_everything_left() {
    let xs = ScsiXfer::new();
    let mut buf = [0u8; 64];
    // SAFETY: `buf` outlives the transfer, which only has its length read.
    unsafe { xs.set_data(buf.as_mut_ptr(), 64) };
    let mut res = VirtioScsiResHdr::new();
    res.response = VIRTIO_SCSI_S_BUSY_FOR_TEST;
    vioscsi_xs_result(&xs, &res);
    assert_eq!(xs.error.get(), XS_DRIVER_STUFFUP);
    assert_eq!(xs.resid.get(), 64);
}

/// `VIRTIO_SCSI_S_BUSY`: any response but OK.
const VIRTIO_SCSI_S_BUSY_FOR_TEST: u8 = crate::dev::pv::vioscsireg::VIRTIO_SCSI_S_BUSY;
