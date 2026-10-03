//! Host tests for rd(4): attach, the spoofed label, and `rdstrategy`'s reads, writes, end of
//! disk and missing unit, over an image in test memory.

use std::boxed::Box;
use std::sync::Once;
use std::vec::Vec;
use std::{assert, assert_eq, vec};

use super::*;
use crate::kern::vfs_subr::tests::setup;
use crate::sys::buf::{B_BUSY, B_DONE, B_WRITE};
use crate::sys::disklabel::{RAW_PART, dl_getpsize, makediskdev};

/// Image sectors.
const SECTORS: usize = 64;

static ATTACH: Once = Once::new();

/// A busy buffer of `len` bytes for `dev`, at `blkno`, reading or writing.
fn buf(dev: Dev, blkno: Daddr, len: usize, read: bool) -> &'static Buf {
    let data: &'static mut Vec<u8> = Box::leak(Box::new(vec![0xeeu8; len]));
    let bp: &'static Buf = Box::leak(Box::new(Buf::new()));
    bp.b_data.set(data.as_mut_ptr());
    bp.b_dev.set(dev);
    bp.b_blkno.set(blkno);
    bp.b_bcount.set(len as i64);
    bp.set(B_BUSY | if read { B_READ } else { B_WRITE });
    bp
}

#[test]
fn rd_reads_and_writes_its_image() {
    let (_g, p) = setup();

    // Sector i holds the byte i.
    let image: &'static mut Vec<u8> = Box::leak(Box::new(vec![0u8; SECTORS * DEV_BSIZE]));
    for (i, s) in image.chunks_mut(DEV_BSIZE).enumerate() {
        s.fill(i as u8);
    }
    // SAFETY: a leaked buffer, used by rd alone from here on.
    unsafe { rd_root_image_set(image.as_mut_ptr(), image.len()) };
    assert_eq!(rd_root_size() as usize, SECTORS * DEV_BSIZE);

    ATTACH.call_once(|| rdattach(NRD));
    let dv = rdlookup(0).expect("rd0 attached");
    let sc = rd_softc(dv);
    assert_eq!(sc.sc_dev.xname(), "rd0");
    assert_eq!(sc.sc_dk.name(), "rd0");
    // SAFETY: the reference `rdlookup` took.
    unsafe { device_unref(dv) };
    assert!(rdlookup(1).is_none());

    // The host has no `readdisklabel`; the label rdstrategy checks against is the spoofed
    // one, published before the read.
    let raw = makediskdev(17, 0, RAW_PART);
    let mut lp = Disklabel::zeroed();
    assert_eq!(rdgetdisklabel(raw, sc, &mut lp, false), Err(Errno::ENODEV));
    assert_eq!(&lp.d_typename[..8], b"RAM disk");
    assert_eq!(lp.d_nsectors as usize, SECTORS);
    let incore = sc.sc_dk.label().expect("in-core label");
    assert_eq!(
        dl_getpsize(&incore.d_partitions[RAW_PART as usize]),
        SECTORS as u64
    );

    // Read sector 2 through rd0c.
    let bp = buf(raw, 2, DEV_BSIZE, true);
    rdstrategy(bp);
    assert!(bp.isset(B_DONE));
    assert!(!bp.isset(B_ERROR));
    assert_eq!(bp.b_resid.get(), 0);
    // SAFETY: the test's own buffer.
    assert!(unsafe { bp.data() }.iter().all(|&b| b == 2));

    // Write sector 3.
    let bp = buf(raw, 3, DEV_BSIZE, false);
    rdstrategy(bp);
    assert!(!bp.isset(B_ERROR));
    assert!(
        image[3 * DEV_BSIZE..4 * DEV_BSIZE]
            .iter()
            .all(|&b| b == 0xee)
    );

    // Past the end: truncated, then end of disk.
    let bp = buf(raw, SECTORS as Daddr - 1, 2 * DEV_BSIZE, true);
    rdstrategy(bp);
    assert_eq!(bp.b_bcount.get(), DEV_BSIZE as i64);
    assert_eq!(bp.b_resid.get(), 0);
    let bp = buf(raw, SECTORS as Daddr, DEV_BSIZE, true);
    rdstrategy(bp);
    assert!(!bp.isset(B_ERROR));
    assert_eq!(bp.b_resid.get(), DEV_BSIZE);

    // No unit 1.
    let bp = buf(makediskdev(17, 1, RAW_PART), 0, DEV_BSIZE, true);
    rdstrategy(bp);
    assert!(bp.isset(B_ERROR));
    assert_eq!(bp.b_error.get(), Some(Errno::ENXIO));

    // DIOCGDINFO copies the in-core label out.
    let mut data = vec![0u8; DISKLABEL_SIZE];
    assert_eq!(rdioctl(raw, DIOCGDINFO, &mut data, 0, p), Ok(()));
    assert_eq!(Disklabel::from_bytes(&data), incore);
    // DIOCSDINFO needs the device open for writing.
    assert_eq!(rdioctl(raw, DIOCSDINFO, &mut data, 0, p), Err(Errno::EBADF));

    assert_eq!(rddump(raw, 0, ptr::null_mut(), 0), Err(Errno::ENXIO));
    assert_eq!(rdsize(raw), -1);
}
