//! Host tests for the disk layer: label checksums and checks (byte-swapped labels too),
//! `bounds_check_with_label`, `readdoslabel` over an in-memory disk (a bare disk with a label
//! at sector 1, an MBR with an OpenBSD partition, a FAT boot sector, a GPT disk with its
//! backup header),
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
fn a_protective_mbr_is_recognised() {
    let mut dp = [DosPartition::default(); NDOSPART];
    dp[0].dp_typ = DOSPTYP_EFI;
    dp[0].dp_start = 1u32.to_le();
    dp[0].dp_size = 1000u32.to_le();
    assert_eq!(gpt_chk_mbr(&dp, 2000), Some(0));
    dp[1].dp_typ = DOSPTYP_OPENBSD;
    dp[1].dp_size = 5u32.to_le();
    assert_eq!(gpt_chk_mbr(&dp, 2000), None); // a hybrid MBR is not protective
}

/// GPT test disk geometry: 256 sectors, the entries at LBA 2..34, usable 34..=222.
const GPT_SECTORS: u64 = 256;
const GPT_LBA_START: u64 = 34;
const GPT_LBA_END: u64 = 222;

/// One 128-byte GPT entry of type `ty` (memory order) over `start..=end`.
fn gpt_entry(ty: [u8; 16], start: u64, end: u64) -> [u8; 128] {
    let mut e = [0u8; 128];
    e[..16].copy_from_slice(&ty);
    e[16] = 0x42; // a non-zero unique GUID
    e[32..40].copy_from_slice(&start.to_le_bytes());
    e[40..48].copy_from_slice(&end.to_le_bytes());
    e
}

/// Writes a GPT header at `lba` whose 128 entries live at `part_lba`, and returns it.
fn gpt_header(lba: u64, alt: u64, part_lba: u64, parts: &[u8]) -> [u8; 92] {
    let mut h = [0u8; 92];
    h[0..8].copy_from_slice(&GPTSIGNATURE.to_le_bytes());
    h[8..12].copy_from_slice(&GPTREVISION.to_le_bytes());
    h[12..16].copy_from_slice(&GPTMINHDRSIZE.to_le_bytes());
    h[24..32].copy_from_slice(&lba.to_le_bytes());
    h[32..40].copy_from_slice(&alt.to_le_bytes());
    h[40..48].copy_from_slice(&GPT_LBA_START.to_le_bytes());
    h[48..56].copy_from_slice(&GPT_LBA_END.to_le_bytes());
    h[72..80].copy_from_slice(&part_lba.to_le_bytes());
    h[80..84].copy_from_slice(&128u32.to_le_bytes());
    h[84..88].copy_from_slice(&GPTMINPARTSIZE.to_le_bytes());
    h[88..92].copy_from_slice(&crc32(0, parts).to_le_bytes());
    let csum = crc32(0, &h);
    h[16..20].copy_from_slice(&csum.to_le_bytes());
    poke(lba as usize * 512, &h);
    h
}

/// A GPT disk: protective MBR, the EFI system partition 34..=49, OpenBSD 64..=222 with a
/// label at its sector 1, primary header at LBA 1 and backup at the last sector.
fn gpt_disk() -> Vec<u8> {
    mbr(&[(DOSPTYP_EFI, 1, GPT_SECTORS as u32 - 1)]);
    let efi_le = [
        0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9,
        0x3b,
    ];
    let obsd_le = [
        0xa0, 0xc7, 0x4c, 0x82, 0xa8, 0x36, 0xe3, 0x11, 0x89, 0x0a, 0x95, 0x25, 0x19, 0xad, 0x3f,
        0x61,
    ];
    let mut parts = vec![0u8; 128 * 128];
    parts[..128].copy_from_slice(&gpt_entry(efi_le, 34, 49));
    parts[128..256].copy_from_slice(&gpt_entry(obsd_le, 64, GPT_LBA_END));
    poke(2 * 512, &parts);
    gpt_header(1, GPT_SECTORS - 1, 2, &parts);
    gpt_header(GPT_SECTORS - 1, 1, 2, &parts);

    let mut inner = label(GPT_SECTORS as u32);
    dl_setpoffset(&mut inner.d_partitions[0], 64);
    dl_setpsize(&mut inner.d_partitions[0], GPT_LBA_END - 64 + 1);
    inner.d_checksum = 0;
    inner.d_checksum = dkcksum(&inner);
    poke(65 * 512, &inner.as_bytes()[..512]);
    parts
}

#[test]
fn a_gpt_puts_the_label_in_the_openbsd_partition() {
    let (_g, bp) = setup(GPT_SECTORS as usize);
    gpt_disk();

    let mut partoff: Daddr = -1;
    let mut lp = spoofed(GPT_SECTORS as u32);
    initdisklabel(&mut lp).expect("geometry");
    assert_eq!(
        readdoslabel(bp, img_strategy, &mut lp, Some(&mut partoff), true),
        Ok(())
    );
    assert_eq!(partoff, 64);

    let mut lp = spoofed(GPT_SECTORS as u32);
    initdisklabel(&mut lp).expect("geometry");
    assert_eq!(readdoslabel(bp, img_strategy, &mut lp, None, true), Ok(()));
    let i = usize::from(b'i' - b'a');
    assert_eq!(lp.d_partitions[i].p_fstype, FS_MSDOS);
    assert_eq!(dl_getpoffset(&lp.d_partitions[i]), 34);
    assert_eq!(dl_getpsize(&lp.d_partitions[i]), 16);
    assert_eq!((dl_getbstart(&lp), dl_getbend(&lp)), (64, GPT_LBA_END + 1));

    let mut lp = spoofed(GPT_SECTORS as u32);
    initdisklabel(&mut lp).expect("geometry");
    assert_eq!(readdoslabel(bp, img_strategy, &mut lp, None, false), Ok(()));
    assert_eq!(dl_getpoffset(&lp.d_partitions[0]), 64);
    assert_eq!(lp.d_partitions[0].p_fstype, FS_BSDFFS);
}

#[test]
fn a_bad_primary_gpt_falls_back_to_the_backup_and_two_bad_ones_fail() {
    let (_g, bp) = setup(GPT_SECTORS as usize);
    gpt_disk();
    poke(512 + 20, &[0xff]); // break the primary header (its checksum no longer matches)

    let mut partoff: Daddr = -1;
    let mut lp = spoofed(GPT_SECTORS as u32);
    initdisklabel(&mut lp).expect("geometry");
    assert_eq!(
        readdoslabel(bp, img_strategy, &mut lp, Some(&mut partoff), true),
        Ok(())
    );
    assert_eq!(partoff, 64);

    // gpt_get_hdr zeroes an invalid header.
    let lp = spoofed(GPT_SECTORS as u32);
    assert_eq!(
        gpt_get_hdr(bp, img_strategy, &lp, 1),
        Ok(GptHeader::default())
    );

    poke((GPT_SECTORS as usize - 1) * 512 + 20, &[0xff]);
    let mut lp = spoofed(GPT_SECTORS as u32);
    initdisklabel(&mut lp).expect("geometry");
    assert_eq!(
        readdoslabel(bp, img_strategy, &mut lp, None, false),
        Err(Errno::ENXIO)
    );
}

#[test]
fn gpt_types_map_to_file_systems() {
    assert_eq!(gpt_get_fstype(&Uuid::default()), FS_UNUSED);
    assert_eq!(gpt_get_fstype(&Uuid::from_bytes(KNOWNFS[1].0)), FS_BSDFFS);
    assert_eq!(gpt_get_fstype(&Uuid::from_bytes([0x5a; 16])), FS_OTHER);
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

/// A leaked disk named `name` whose label has the DUID `uid`, on `DISKLIST`.
fn listed_disk(name: &[u8], uid: [u8; DUID_SIZE]) -> &'static Disk {
    let dk: &'static Disk = Box::leak(Box::new(Disk::new()));
    let mut n = [0u8; crate::sys::disk::DS_DISKNAMELEN];
    n[..name.len()].copy_from_slice(name);
    dk.dk_name.set(n);
    let lp: &'static mut Disklabel = Box::leak(Box::new(label(64)));
    lp.d_uid = uid;
    dk.dk_label.set(Some(NonNull::from(lp)));
    // SAFETY: a fresh disk, on no list; the test lock keeps other tests off DISKLIST.
    unsafe { DISKLIST.0.insert_tail(dk) };
    dk
}

/// `disk_map(path, flags)` as a string, `None` for the C's -1.
fn map(path: &[u8], flags: i32) -> Option<std::string::String> {
    let mut out = [0xffu8; 90];
    if !disk_map(path, &mut out, flags) {
        return None;
    }
    let n = out.iter().position(|&c| c == 0).unwrap_or(out.len());
    Some(std::string::String::from_utf8_lossy(&out[..n]).into_owned())
}

#[test]
fn disk_map_finds_a_disk_by_its_duid() {
    let (_g, _p) = crate::kern::vfs_subr::tests::setup();
    let uid = [0x4a, 0x5b, 0x6c, 0x7d, 0x8e, 0x9f, 0x01, 0x23];
    let dk = listed_disk(b"sd3", uid);

    let found = |path: &[u8], flags| map(path, flags);
    assert_eq!(
        found(b"4a5b6c7d8e9f0123.a\0junk", DM_OPENBLCK).as_deref(),
        Some("/dev/sd3a")
    );
    assert_eq!(
        found(b"4a5b6c7d8e9f0123.d", 0).as_deref(),
        Some("/dev/rsd3d")
    );
    assert_eq!(
        found(b"4a5b6c7d8e9f0123", DM_OPENPART).as_deref(),
        Some("/dev/rsd3c")
    );
    assert_eq!(
        found(b"4a5b6c7d8e9f0123.e", DM_OPENPART).as_deref(),
        Some("/dev/rsd3c")
    );
    // Truncated to the buffer, as snprintf does.
    let mut small = [0xffu8; 6];
    assert!(disk_map(b"4a5b6c7d8e9f0123.a", &mut small, DM_OPENBLCK));
    assert_eq!(&small, b"/dev/\0");

    // Not a DUID name.
    for bad in [
        &b"/dev/sd3a"[..],
        b"4a5b6c7d8e9f0123",
        b"4a5b6c7d8e9f0123:a",
        b"4a5b6c7d8e9f0123.a/",
        b"4A5B6C7D8E9F0123.a",
        b"4a5b6c7d8e9f0123.?",
        // No such disk.
        b"4a5b6c7d8e9f0124.a",
    ] {
        assert_eq!(
            map(bad, 0),
            None,
            "{}",
            std::string::String::from_utf8_lossy(bad)
        );
    }

    // Fail if there are duplicate UIDs!
    let twin = listed_disk(b"sd4", uid);
    assert_eq!(map(b"4a5b6c7d8e9f0123.a", 0), None);

    // SAFETY: both are on DISKLIST, inserted above under the same lock.
    unsafe {
        DISKLIST.0.remove(twin);
        DISKLIST.0.remove(dk);
    }
}
