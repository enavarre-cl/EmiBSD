//! Host tests for the date conversions, the inode numbers of records and the attributes of
//! plain ISO 9660 records.

use super::*;
use crate::isofs::cd9660::cd9660_extern::ISO_FTYPE_DEFAULT;
use crate::isofs::cd9660::iso::tests::{image, rec, test_mnt, test_node};
use crate::sys::stat::S_IFMT;

/// `cd9660_tstamp_conv7` of a 7-byte date.
fn conv7(d: [u8; 7]) -> (bool, Timespec) {
    let mut t = Timespec::new(-1, -1);
    let ok = cd9660_tstamp_conv7(&d, &mut t);
    (ok, t)
}

/// `cd9660_tstamp_conv17` of a 17-byte date.
fn conv17(d: &[u8; 17]) -> (bool, Timespec) {
    let mut t = Timespec::new(-1, -1);
    let ok = cd9660_tstamp_conv17(d, &mut t);
    (ok, t)
}

#[test]
fn conv7_counts_seconds_since_the_epoch() {
    assert_eq!(conv7([70, 1, 1, 0, 0, 0, 0]), (true, Timespec::new(0, 0)));
    assert_eq!(
        conv7([99, 12, 31, 23, 59, 59, 0]),
        (true, Timespec::new(946_684_799, 0))
    );
    assert_eq!(
        conv7([100, 2, 29, 12, 0, 0, 0]),
        (true, Timespec::new(951_825_600, 0))
    );
    // the offset from GMT is in 15-minute units: local 07:36:48 at -3h is 10:36:48 GMT
    assert_eq!(
        conv7([126, 10, 4, 7, 36, 48, (-12i8) as u8]),
        (true, Timespec::new(1_791_110_208, 0))
    );
    // an offset outside -48..=52 is unreliable and ignored
    assert_eq!(
        conv7([126, 10, 4, 7, 36, 48, 100]),
        (true, Timespec::new(1_791_099_408, 0))
    );
    // before 1970: the epoch, and false
    assert_eq!(
        conv7([69, 12, 31, 0, 0, 0, 0]),
        (false, Timespec::new(0, 0))
    );
}

#[test]
fn conv17_reads_the_digits() {
    assert_eq!(
        conv17(b"2026100407364800\xf4"),
        (true, Timespec::new(1_791_110_208, 0))
    );
    assert_eq!(conv17(b"1970010100000000\x00"), (true, Timespec::new(0, 0)));
    assert_eq!(
        conv17(b"1969123123595900\x00"),
        (false, Timespec::new(0, 0))
    );
}

#[test]
fn isodirino_is_the_byte_offset_of_the_data() {
    let imp = test_mnt(ISO_FTYPE_DEFAULT);
    assert_eq!(isodirino(&rec(&image::SUB_REC), imp), 21 << 11);
    assert_eq!(isodirino(&rec(&image::ROOT_DOT), imp), 20 << 11);
    // the extended attribute record comes first
    let mut r = image::FILE_REC;
    r[1] = 2;
    assert_eq!(isodirino(&rec(&r), imp), (23 + 2) << 11);
}

#[test]
fn plain_records_get_default_attributes_and_times() {
    let imp = test_mnt(ISO_FTYPE_DEFAULT);
    let (ip, _vp) = test_node(imp);

    cd9660_defattr(&rec(&image::SUB_REC), ip, None);
    let ino = ip.inode.get();
    assert_eq!(u32::from(ino.iso_mode) & S_IFMT, S_IFDIR);
    assert_eq!(u32::from(ino.iso_mode) & 0o7777, 0o555);
    assert_eq!((ino.iso_links, ino.iso_uid, ino.iso_gid), (1, 0, 0));

    cd9660_defattr(&rec(&image::FILE_REC), ip, None);
    cd9660_deftstamp(&rec(&image::FILE_REC), ip, None);
    let ino = ip.inode.get();
    assert_eq!(u32::from(ino.iso_mode), S_IFREG | 0o555);
    assert_eq!(ino.iso_ctime, Timespec::new(1_791_110_208, 0));
    assert_eq!(ino.iso_atime, ino.iso_ctime);
    assert_eq!(ino.iso_mtime, ino.iso_ctime);
}

#[test]
fn a_new_node_is_unhashed_and_zeroed() {
    let ip = IsoNode::new();
    assert!(!ip.i_hashed.get());
    assert_eq!(ip.inode.get(), IsoRripInode::default());
    ip.update_inode(|i| i.iso_mode = 0o40555);
    assert_eq!(ip.inode.get().iso_mode, 0o40555);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/isofs/cd9660/cd9660_node.h");
    assert_eq!(
        crate::reftest::int(&defs, "IN_ACCESS"),
        Some(i64::from(IN_ACCESS))
    );
}
