//! Host tests for the disk layer: label checksums and checks (byte-swapped labels too),
//! `bounds_check_with_label`, `readdoslabel` over an in-memory disk (a bare disk with a label
//! at sector 1, an MBR with an OpenBSD partition, a FAT boot sector, a protective GPT MBR),
//! `setdisklabel`, the open masks and the DUID helpers.

use std::boxed::Box;
use std::sync::{Mutex, MutexGuard};
use std::vec::Vec;
use std::{assert, assert_eq, vec};

use super::*;
use crate::kern::vfs_bio::biodone;
use crate::machine::intr::{splbio, splx};
use crate::sys::disklabel::{DOSPARTOFF, DOSPTYP_FAT32, FS_SWAP};

/// The in-memory disk the strategy below reads and writes, in `DEV_BSIZE` sectors.
static IMG: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// A strategy over `IMG`: a synchronous transfer at `b_blkno`, then `biodone`.
fn img_strategy(bp: &'static Buf) {
    let off = bp.b_blkno.get() as usize * DEV_BSIZE;
    let len = bp.b_bcount.get() as usize;
    {
        let mut img = IMG.lock().unwrap_or_else(|e| e.into_inner());
        if off + len > img.len() {
            bp.b_error.set(Some(Errno::EIO));
            bp.set(B_ERROR);
        } else {
            // SAFETY: the buffer is busy for this transfer and mapped.
            let data = unsafe { bp.data() };
            if bp.isset(B_READ) {
                data.copy_from_slice(&img[off..off + len]);
            } else {
                img[off..off + len].copy_from_slice(data);
            }
            bp.b_resid.set(0);
        }
    }
    let s = splbio();
    biodone(bp);
    splx(s);
}

/// The global test lock, a disk of `sectors` zero sectors, and a busy buffer of one page.
fn setup(sectors: usize) -> (MutexGuard<'static, ()>, &'static Buf) {
    let (g, _p) = crate::kern::vfs_subr::tests::setup();
    *IMG.lock().unwrap_or_else(|e| e.into_inner()) = vec![0u8; sectors * DEV_BSIZE];
    let data: &'static mut [u8; 4096] = Box::leak(Box::new([0u8; 4096]));
    let bp: &'static Buf = Box::leak(Box::new(Buf::new()));
    bp.b_data.set(data.as_mut_ptr());
    bp.b_dev.set(makediskdev(17, 0, RAW_PART));
    (g, bp)
}

/// Writes `bytes` at byte offset `off` of the disk.
fn poke(off: usize, bytes: &[u8]) {
    IMG.lock().unwrap_or_else(|e| e.into_inner())[off..off + bytes.len()].copy_from_slice(bytes);
}

/// A valid label for a disk of `sectors`: `a` is FFS over the whole disk, `b` swap.
fn label(sectors: u32) -> Disklabel {
    let mut d = Disklabel::zeroed();
    d.d_magic = DISKMAGIC;
    d.d_magic2 = DISKMAGIC;
    d.d_secsize = 512;
    d.d_nsectors = sectors;
    d.d_ntracks = 1;
    d.d_ncylinders = 1;
    d.d_secpercyl = sectors;
    d.d_secperunit = sectors;
    d.d_version = 1;
    d.d_npartitions = 3;
    dl_setpsize(&mut d.d_partitions[0], u64::from(sectors));
    d.d_partitions[0].p_fstype = FS_BSDFFS;
    dl_setpsize(&mut d.d_partitions[1], 8);
    dl_setpoffset(&mut d.d_partitions[1], 8);
    d.d_partitions[1].p_fstype = FS_SWAP;
    dl_setpsize(&mut d.d_partitions[2], u64::from(sectors));
    d.d_checksum = dkcksum(&d);
    d
}

/// The label a driver spoofs for a disk of `sectors` before reading (as rd(4) does).
fn spoofed(sectors: u32) -> Disklabel {
    let mut d = Disklabel::zeroed();
    d.d_secsize = 512;
    d.d_nsectors = sectors;
    d.d_ntracks = 1;
    d.d_ncylinders = 1;
    d.d_secpercyl = sectors;
    dl_setdsize(&mut d, u64::from(sectors));
    d.d_version = 1;
    d
}

#[test]
fn a_label_with_its_checksum_sums_to_zero() {
    let d = label(64);
    assert_eq!(dkcksum(&d), 0);
    let mut bad = d;
    bad.d_ntracks = 2;
    assert!(dkcksum(&bad) != 0);
}

#[test]
fn initdisklabel_makes_the_raw_partition_the_disk() {
    let mut d = spoofed(100);
    d.d_partitions[0].p_size = 5;
    assert_eq!(initdisklabel(&mut d), Ok(()));
    assert_eq!(usize::from(d.d_npartitions), MAXPARTITIONS);
    assert_eq!(dl_getpsize(&d.d_partitions[0]), 0);
    assert_eq!(dl_getpsize(&d.d_partitions[RAW_PART as usize]), 100);
    assert_eq!(dl_getbend(&d), 100);
    d.d_secpercyl = 0;
    assert_eq!(initdisklabel(&mut d), Err(Errno::ERANGE));
}

#[test]
fn checkdisklabel_accepts_native_and_byte_swapped_labels() {
    let mut lp = spoofed(64);
    let mut dlp = label(64);
    assert_eq!(checkdisklabel(0, &mut dlp, &mut lp, 0, 1000), Ok(()));
    assert_eq!(lp.d_partitions[0].p_fstype, FS_BSDFFS);
    assert_eq!(dl_getbend(&lp), 64); // clamped to the disk
    assert_eq!(dkcksum(&lp), 0);

    // The same label written by a machine of the other byte order.
    let native = label(64);
    let mut swapped = native;
    swapped.d_magic = native.d_magic.swap_bytes();
    swapped.d_magic2 = native.d_magic2.swap_bytes();
    swapped.d_secsize = native.d_secsize.swap_bytes();
    swapped.d_nsectors = native.d_nsectors.swap_bytes();
    swapped.d_ntracks = native.d_ntracks.swap_bytes();
    swapped.d_ncylinders = native.d_ncylinders.swap_bytes();
    swapped.d_secpercyl = native.d_secpercyl.swap_bytes();
    swapped.d_secperunit = native.d_secperunit.swap_bytes();
    swapped.d_version = native.d_version.swap_bytes();
    swapped.d_npartitions = native.d_npartitions.swap_bytes();
    for p in swapped.d_partitions.iter_mut().take(3) {
        p.p_size = p.p_size.swap_bytes();
        p.p_offset = p.p_offset.swap_bytes();
    }
    // The checksum over the swapped words, as the other machine computed it.
    swapped.d_checksum = 0;
    let sum = swapped
        .cksum_words(usize::from(swapped.d_npartitions.swap_bytes()))
        .fold(0u16, |s, w| s ^ w);
    swapped.d_checksum = sum;
    let mut lp = spoofed(64);
    assert_eq!(checkdisklabel(0, &mut swapped, &mut lp, 0, 64), Ok(()));
    assert_eq!(lp.d_secsize, 512);
    assert_eq!(dl_getpsize(&lp.d_partitions[1]), 8);
    assert_eq!(dl_getpoffset(&lp.d_partitions[1]), 8);

    let mut zero = Disklabel::zeroed();
    assert_eq!(
        checkdisklabel(0, &mut zero, &mut lp, 0, 64),
        Err(Errno::EINVAL)
    );
    let mut nomagic = spoofed(64);
    assert_eq!(
        checkdisklabel(0, &mut nomagic, &mut lp, 0, 64),
        Err(Errno::ENOENT)
    );
}

#[test]
fn bounds_check_truncates_and_rejects() {
    let lp = label(64);
    let bp = Buf::new();
    bp.b_dev.set(makediskdev(17, 0, 0));

    bp.b_blkno.set(60);
    bp.b_bcount.set(8 * 512);
    assert!(bounds_check_with_label(&bp, &lp));
    assert_eq!(bp.b_bcount.get(), 4 * 512); // truncated at the end of `a`

    bp.b_blkno.set(64);
    bp.b_bcount.set(512);
    assert!(!bounds_check_with_label(&bp, &lp)); // EOF
    assert!(!bp.isset(B_ERROR));
    assert_eq!(bp.b_resid.get(), 512);

    bp.b_blkno.set(65);
    assert!(!bounds_check_with_label(&bp, &lp));
    assert!(bp.isset(B_ERROR));
    assert_eq!(bp.b_error.get(), Some(Errno::EINVAL));

    let bp = Buf::new();
    bp.b_bcount.set(100); // not a whole sector
    assert!(!bounds_check_with_label(&bp, &lp));
    assert!(bp.isset(B_ERROR));
}

#[test]
fn readdoslabel_reads_the_label_of_a_bare_disk() {
    let (_g, bp) = setup(64);
    poke(512, &label(64).as_bytes()[..512]);

    let mut lp = spoofed(64);
    initdisklabel(&mut lp).expect("geometry");
    assert_eq!(readdoslabel(bp, img_strategy, &mut lp, None, false), Ok(()));
    assert_eq!(lp.d_magic, DISKMAGIC);
    assert_eq!(lp.d_partitions[0].p_fstype, FS_BSDFFS);
    assert_eq!(dl_getpsize(&lp.d_partitions[0]), 64);
    assert_eq!(dl_getpsize(&lp.d_partitions[RAW_PART as usize]), 64);
    assert_eq!(dkcksum(&lp), 0);

    // No label at sector 1: the spoofed raw partition only, and the check's error.
    poke(512, &[0u8; 512]);
    let mut lp = spoofed(64);
    initdisklabel(&mut lp).expect("geometry");
    assert_eq!(
        readdoslabel(bp, img_strategy, &mut lp, None, false),
        Err(Errno::EINVAL)
    );
    // spoofonly stops before reading the label.
    let mut lp = spoofed(64);
    initdisklabel(&mut lp).expect("geometry");
    assert_eq!(readdoslabel(bp, img_strategy, &mut lp, None, true), Ok(()));
    assert_eq!(dl_getpsize(&lp.d_partitions[RAW_PART as usize]), 64);
    assert_eq!(dl_getpsize(&lp.d_partitions[0]), 0);
}

/// An MBR at sector 0 with the given `(type, start, size)` entries and its signature.
fn mbr(entries: &[(u8, u32, u32)]) {
    let mut s = [0u8; 512];
    for (i, &(typ, start, size)) in entries.iter().enumerate() {
        let e = DOSPARTOFF + 16 * i;
        s[e + 4] = typ;
        s[e + 8..e + 12].copy_from_slice(&start.to_le_bytes());
        s[e + 12..e + 16].copy_from_slice(&size.to_le_bytes());
    }
    s[510..512].copy_from_slice(&DOSMBR_SIGNATURE.to_le_bytes());
    poke(0, &s);
}

#[test]
fn an_mbr_puts_the_label_in_the_openbsd_partition() {
    let (_g, bp) = setup(256);
    mbr(&[(DOSPTYP_FAT32, 1, 31), (DOSPTYP_OPENBSD, 64, 192)]);
    // The label lives at sector 1 of the A6 partition.
    let mut inner = label(192);
    dl_setpoffset(&mut inner.d_partitions[0], 64);
    inner.d_checksum = 0;
    inner.d_checksum = dkcksum(&inner);
    poke(65 * 512, &inner.as_bytes()[..512]);

    let mut partoff: Daddr = -1;
    let mut lp = spoofed(256);
    initdisklabel(&mut lp).expect("geometry");
    assert_eq!(
        readdoslabel(bp, img_strategy, &mut lp, Some(&mut partoff), true),
        Ok(())
    );
    assert_eq!(partoff, 64);

    let mut lp = spoofed(256);
    initdisklabel(&mut lp).expect("geometry");
    assert_eq!(readdoslabel(bp, img_strategy, &mut lp, None, true), Ok(()));
    // The FAT partition is spoofed as `i`, the OpenBSD one bounds the label.
    let i = usize::from(b'i' - b'a');
    assert_eq!(lp.d_partitions[i].p_fstype, FS_MSDOS);
    assert_eq!(dl_getpoffset(&lp.d_partitions[i]), 1);
    assert_eq!(dl_getpsize(&lp.d_partitions[i]), 31);
    assert_eq!((dl_getbstart(&lp), dl_getbend(&lp)), (64, 256));

    let mut lp = spoofed(256);
    initdisklabel(&mut lp).expect("geometry");
    assert_eq!(readdoslabel(bp, img_strategy, &mut lp, None, false), Ok(()));
    assert_eq!(dl_getpoffset(&lp.d_partitions[0]), 64);
    assert_eq!(lp.d_partitions[0].p_fstype, FS_BSDFFS);
}

#[test]
fn a_fat_boot_sector_spoofs_partition_i_without_a_label() {
    let mut s = [0u8; DEV_BSIZE];
    s[0] = 0xeb;
    s[2] = 0x90;
    s[11..13].copy_from_slice(&512u16.to_le_bytes());
    s[16] = 2;
    let mut lp = spoofed(64);
    initdisklabel(&mut lp).expect("geometry");
    let mut partoff: Daddr = 0;
    spooffat(&s, &mut lp, &mut partoff);
    assert_eq!(partoff, -1);
    assert_eq!(lp.d_magic, DISKMAGIC);
    let i = usize::from(b'i' - b'a');
    assert_eq!(lp.d_partitions[i].p_fstype, FS_MSDOS);
    assert_eq!(dl_getpsize(&lp.d_partitions[i]), 64);
}

#[test]
fn a_protective_mbr_is_recognised_and_gpt_reported() {
    let mut dp = [DosPartition::default(); NDOSPART];
    dp[0].dp_typ = DOSPTYP_EFI;
    dp[0].dp_start = 1u32.to_le();
    dp[0].dp_size = 1000u32.to_le();
    assert_eq!(gpt_chk_mbr(&dp, 2000), Some(0));
    dp[1].dp_typ = DOSPTYP_OPENBSD;
    dp[1].dp_size = 5u32.to_le();
    assert_eq!(gpt_chk_mbr(&dp, 2000), None); // a hybrid MBR is not protective

    let (_g, bp) = setup(64);
    mbr(&[(DOSPTYP_EFI, 1, 63)]);
    let mut lp = spoofed(64);
    initdisklabel(&mut lp).expect("geometry");
    assert_eq!(
        readdoslabel(bp, img_strategy, &mut lp, None, false),
        Err(Errno::ENOSYS)
    );
}

#[test]
fn mbr_types_map_to_file_systems() {
    assert_eq!(mbr_get_fstype(DOSPTYP_OPENBSD), FS_BSDFFS);
    assert_eq!(mbr_get_fstype(DOSPTYP_LINUX), FS_EXT2FS);
    assert_eq!(mbr_get_fstype(DOSPTYP_EFISYS), FS_MSDOS);
    assert_eq!(mbr_get_fstype(DOSPTYP_EXTEND), FS_OTHER);
    assert_eq!(mbr_get_fstype(DOSPTYP_UNUSED), FS_UNUSED);
}

#[test]
fn setdisklabel_checks_and_keeps_the_disk_size() {
    let (_g, _bp) = setup(1);
    let mut olp = label(64);
    let mut nlp = label(64);
    dl_setdsize(&mut nlp, 1); // ignored: the disk size is preserved
    nlp.d_checksum = 0;
    nlp.d_checksum = dkcksum(&nlp);
    assert_eq!(setdisklabel(&mut olp, &mut nlp, 0), Ok(()));
    assert_eq!(dl_getdsize(&olp), 64);
    assert!(!duid_iszero(&olp.d_uid)); // a DUID was generated
    assert_eq!(dkcksum(&olp), 0);

    // Shrinking an open partition is refused.
    let mut nlp = label(64);
    dl_setpsize(&mut nlp.d_partitions[0], 10);
    nlp.d_checksum = 0;
    nlp.d_checksum = dkcksum(&nlp);
    assert_eq!(setdisklabel(&mut olp, &mut nlp, 1), Err(Errno::EBUSY));

    let mut bad = label(64);
    bad.d_secsize = 100;
    assert_eq!(setdisklabel(&mut olp, &mut bad, 0), Err(Errno::EINVAL));
}

#[test]
fn open_masks_follow_opens_and_closes() {
    let dk = Disk::new();
    let mut lp = label(64);
    dk.dk_label.set(Some(NonNull::from(&mut lp)));
    assert_eq!(disk_openpart(&dk, 0, S_IFBLK as i32, true), Ok(()));
    assert_eq!(disk_openpart(&dk, 2, S_IFCHR as i32, false), Ok(()));
    assert_eq!(
        disk_openpart(&dk, 5, S_IFBLK as i32, true),
        Err(Errno::ENXIO)
    );
    assert_eq!(
        disk_openpart(&dk, 0, S_IFBLK as i32, false),
        Err(Errno::ENXIO)
    );
    assert_eq!(dk.dk_openmask.get(), 0b101);
    disk_closepart(&dk, 0, S_IFBLK as i32);
    assert_eq!(dk.dk_openmask.get(), 0b100);
    dk.dk_label.set(None);
}

#[test]
fn duids_format_as_hex() {
    let duid = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
    assert_eq!(&duid_format(&duid), b"0123456789abcdef");
    assert!(duid_iszero(&[0; DUID_SIZE]));
    assert!(!duid_iszero(&duid));
    assert!(duid_equal(&duid, &duid));
}

#[test]
fn readlabel_errors_print_the_c_messages() {
    let e = DiskReadlabelError::Open {
        dev: 0x1100,
        rawdev: 0x2f02,
        error: Errno::ENXIO,
    };
    assert_eq!(
        std::format!("{e}"),
        "cannot open disk, 0x1100/0x2f02, error 6"
    );
}
