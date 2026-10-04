//! Host tests for `scsiconf.rs`.

use std::vec::Vec;
use std::{assert, assert_eq, vec};

use super::*;
use crate::scsi::scsi_all::{INQUIRY, ScsiInquiry};

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/scsi/scsiconf.h");
    let ours = crate::reftest::assert_defines!(defs;
        DEVID_NONE, DEVID_NAA, DEVID_EUI, DEVID_T10, DEVID_SERIAL, DEVID_WWN, DEVID_F_PRINT,
        SDEV_S_DYING, SDEV_REMOVABLE, SDEV_MEDIA_LOADED, SDEV_READONLY, SDEV_OPEN, SDEV_DBX,
        SDEV_EJECTING, SDEV_ATAPI, SDEV_UMASS, SDEV_VIRTUAL, SDEV_OWN_IOPL, SDEV_UFI,
        SDEV_AUTOSAVE, SDEV_NOSYNC, SDEV_NOWIDE, SDEV_NOTAGS, SDEV_NOSYNCCACHE, ADEV_NOSENSE,
        ADEV_LITTLETOC, ADEV_NOCAPACITY, ADEV_NODOORLOCK, SDEV_NO_ADAPTER_TARGET,
        SCSI_NOSLEEP, SCSI_POLL, SCSI_AUTOCONF, ITSDONE, SCSI_SILENT, SCSI_IGNORE_NOT_READY,
        SCSI_IGNORE_MEDIA_CHANGE, SCSI_IGNORE_ILLEGAL_REQUEST, SCSI_RESET, SCSI_DATA_IN,
        SCSI_DATA_OUT, SCSI_TARGET, SCSI_ESCAPE, SCSI_PRIVATE, SCSI_OP_TARGET, SCSI_OP_RESET,
        SCSI_OP_BDINFO, XS_NOERROR, XS_SENSE, XS_DRIVER_STUFFUP, XS_SELTIMEOUT, XS_TIMEOUT,
        XS_BUSY, XS_SHORTSENSE, XS_RESET, TEST_READY_RETRIES, SCSI_RETRIES, SCSI_REV_0,
        SCSI_REV_1, SCSI_REV_2, SCSI_REV_SPC, SCSI_REV_SPC2, SCSI_REV_SPC3, SCSI_REV_SPC4,
        SCSI_REV_SPC5,
    );
    crate::reftest::assert_complete(&defs, "XS_", &ours);
    crate::reftest::assert_complete(&defs, "SCSI_REV_", &ours);
    assert_eq!(SCSI_IOPOOL_POISON.as_ptr() as usize, 0x5c5);
}

#[test]
fn big_endian_helpers_round_trip() {
    let mut b = [0u8; 8];
    _lto2b(0x1234, &mut b);
    assert_eq!(&b[..2], &[0x12, 0x34]);
    assert_eq!(_2btol(&b), 0x1234);
    _lto3b(0x00ab_cdef, &mut b);
    assert_eq!(&b[..3], &[0xab, 0xcd, 0xef]);
    assert_eq!(_3btol(&b), 0x00ab_cdef);
    _lto4b(0xdead_beef, &mut b);
    assert_eq!(&b[..4], &[0xde, 0xad, 0xbe, 0xef]);
    assert_eq!(_4btol(&b), 0xdead_beef);
    _lto8b(0x0102_0304_0506_0708, &mut b);
    assert_eq!(b, [1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(_8btol(&b), 0x0102_0304_0506_0708);
    assert_eq!(_5btol(&b), 0x01_0203_0405);
    // The high bits beyond the field are dropped, as the C's masks do.
    _lto2b(0xffff_1234, &mut b);
    assert_eq!(_2btol(&b), 0x1234);
}

/// A `devid_alloc`-shaped allocation: the header and the identifier bytes after it.
fn devid(d_type: u8, id: &[u8]) -> Vec<u8> {
    let mut v = vec![d_type, 0, 1, id.len() as u8];
    v.extend_from_slice(id);
    v
}

fn as_devid(v: &[u8]) -> &Devid {
    // SAFETY: `Devid` is four bytes with alignment 1, and the vector holds the header and
    // `d_len` bytes after it, like a `devid_alloc` allocation.
    unsafe { &*v.as_ptr().cast::<Devid>() }
}

#[test]
fn devid_cmp_compares_type_length_and_bytes() {
    let a = devid(DEVID_NAA, &[1, 2, 3, 4]);
    let b = devid(DEVID_NAA, &[1, 2, 3, 4]);
    let c = devid(DEVID_NAA, &[1, 2, 3, 5]);
    let d = devid(DEVID_EUI, &[1, 2, 3, 4]);
    let n = devid(DEVID_NONE, &[1, 2, 3, 4]);
    let n2 = devid(DEVID_NONE, &[1, 2, 3, 4]);
    // SAFETY: every argument is a `devid`-shaped allocation (see `devid`).
    unsafe {
        assert!(devid_cmp(Some(as_devid(&a)), Some(as_devid(&b))));
        assert!(!devid_cmp(Some(as_devid(&a)), Some(as_devid(&c))));
        assert!(!devid_cmp(Some(as_devid(&a)), Some(as_devid(&d))));
        assert!(!devid_cmp(Some(as_devid(&n)), Some(as_devid(&n2))));
        assert!(devid_cmp(Some(as_devid(&n)), Some(as_devid(&n))));
        assert!(!devid_cmp(None, Some(as_devid(&a))));
        assert!(!devid_cmp(Some(as_devid(&a)), None));
        assert_eq!(as_devid(&a).id(), &[1, 2, 3, 4]);
    }
}

#[test]
fn xfer_cmd_views() {
    let xs = ScsiXfer::new();
    xs.with_cmd(|cmd: &mut ScsiInquiry| {
        cmd.opcode = INQUIRY;
        _lto2b(36, &mut cmd.length);
    });
    assert_eq!(xs.cmd.get().opcode, INQUIRY);
    let inq: ScsiInquiry = xs.cmd_as();
    assert_eq!(_2btol(&inq.length), 36);

    let mut cdb = ScsiInquiry::default();
    cdb.opcode = 0x55;
    xs.set_cmd(&cdb);
    assert_eq!(xs.cmd.get().opcode, 0x55);
    // The bytes past the CDB are left alone.
    assert_eq!(xs.cmd.get().bytes[5], 0);
}

#[test]
fn xfer_data() {
    let xs = ScsiXfer::new();
    let mut buf = [0u8; 4];
    // SAFETY: `buf` outlives every use of the data below and nothing else touches it.
    unsafe { xs.set_data(buf.as_mut_ptr(), 4) };
    assert_eq!(xs.datalen(), 4);
    // SAFETY: this test is the transfer's only user.
    unsafe { xs.data_slice()[1] = 7 };
    xs.clear_data();
    // SAFETY: as above.
    assert!(unsafe { xs.data_slice() }.is_empty());
    assert_eq!(buf, [0, 7, 0, 0]);
}

#[test]
fn inquiry_revision_helpers() {
    let mut inq = crate::scsi::scsi_all::ScsiInquiryData::new();
    inq.version = 0x05 | 0x08;
    inq.response_format = 0x12;
    assert_eq!(sid_ansii_rev(&inq), SCSI_REV_SPC3);
    assert_eq!(sid_response_format(&inq), 0x02);
}

#[test]
fn new_link_uses_the_default_sense_handler() {
    let link = ScsiLink::new();
    assert!(core::ptr::fn_addr_eq(
        link.interpret_sense.get(),
        scsi_interpret_sense as fn(&'static ScsiXfer) -> Result<(), Errno>
    ));
    assert!(link.pool.get().is_none());
    assert!(link.id().is_none());
}
