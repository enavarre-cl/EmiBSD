//! Host tests of `denode.rs`: the transfer of directory entries between the on-disk and the
//! in-core form, `DETIMES`, and the file handle.

use super::*;
use crate::msdosfs::direntry::ATTR_READONLY;
use crate::msdosfs::fat::{FAT16_MASK, FAT32_MASK};
use std::boxed::Box;

fn pmp(fatmask: u32) -> &'static Msdosfsmount {
    let pmp: &'static Msdosfsmount = Box::leak(Box::new(Msdosfsmount::new()));
    pmp.pm_fatmask.set(fatmask);
    pmp
}

/// A directory entry for "HELLO.TXT", 1234 bytes from cluster 0x12345.
fn entry() -> [u8; 32] {
    let mut e = [0u8; 32];
    e[..11].copy_from_slice(b"HELLO   TXT");
    e[11] = ATTR_READONLY;
    e[13] = 57;
    e[14..16].copy_from_slice(&0x6000u16.to_le_bytes()); // CTime
    e[16..18].copy_from_slice(&0x5021u16.to_le_bytes()); // CDate
    e[18..20].copy_from_slice(&0x5022u16.to_le_bytes()); // ADate
    e[20..22].copy_from_slice(&0x0001u16.to_le_bytes()); // HighClust
    e[22..24].copy_from_slice(&0x6001u16.to_le_bytes()); // MTime
    e[24..26].copy_from_slice(&0x5023u16.to_le_bytes()); // MDate
    e[26..28].copy_from_slice(&0x2345u16.to_le_bytes()); // StartCluster
    e[28..32].copy_from_slice(&1234u32.to_le_bytes());
    e
}

#[test]
fn internalize_fat16_ignores_the_high_cluster() {
    let dep = Denode::new();
    dep.de_pmp.set(Some(pmp(FAT16_MASK)));
    let e = entry();
    de_internalize(&dep, Direntry::at(&e, 0));
    assert_eq!(&dep.de_Name.get(), b"HELLO   TXT");
    assert_eq!(dep.de_Attributes.get(), ATTR_READONLY);
    assert_eq!(dep.de_CTimeHundredth.get(), 57);
    assert_eq!(dep.de_CTime.get(), 0x6000);
    assert_eq!(dep.de_CDate.get(), 0x5021);
    assert_eq!(dep.de_ADate.get(), 0x5022);
    assert_eq!(dep.de_MTime.get(), 0x6001);
    assert_eq!(dep.de_MDate.get(), 0x5023);
    assert_eq!(dep.de_StartCluster.get(), 0x2345);
    assert_eq!(dep.de_FileSize.get(), 1234);

    let mut out = [0xaau8; 32];
    de_externalize(Direntry::at_mut(&mut out, 0), &dep);
    let mut want = entry();
    want[12] = CASE_LOWER_BASE | CASE_LOWER_EXT;
    want[20..22].copy_from_slice(&[0, 0]);
    assert_eq!(out, want);
}

#[test]
fn internalize_fat32_round_trip() {
    let dep = Denode::new();
    dep.de_pmp.set(Some(pmp(FAT32_MASK)));
    let e = entry();
    de_internalize(&dep, Direntry::at(&e, 0));
    assert_eq!(dep.de_StartCluster.get(), 0x12345);
    let mut out = [0u8; 32];
    de_externalize(Direntry::at_mut(&mut out, 0), &dep);
    let mut want = entry();
    want[12] = CASE_LOWER_BASE | CASE_LOWER_EXT;
    assert_eq!(out, want);

    // A directory's entry has size 0 on disk.
    dep.de_Attributes.set(ATTR_DIRECTORY);
    de_externalize(Direntry::at_mut(&mut out, 0), &dep);
    assert_eq!(&out[28..32], &[0, 0, 0, 0]);
}

#[test]
fn detimes_turns_requests_into_dos_times() {
    let pmp = pmp(FAT16_MASK);
    let dep = Denode::new();
    dep.de_pmp.set(Some(pmp));
    // 2026-10-04 12:34:56, and 1980-01-01 00:00:00.
    let now = Timespec {
        tv_sec: 1_791_117_296,
        tv_nsec: 0,
    };
    let epoch = Timespec {
        tv_sec: 315_532_800,
        tv_nsec: 0,
    };

    // No request: nothing changes.
    detimes(&dep, &now, &now, &now);
    assert_eq!(dep.de_flag.get(), 0);

    dep.set_flag(DE_UPDATE | DE_ACCESS | DE_CREATE);
    detimes(&dep, &epoch, &now, &epoch);
    assert_eq!(dep.de_flag.get(), DE_MODIFIED);
    assert_eq!(dep.de_MDate.get(), 4 | 10 << 5 | 46 << 9);
    assert_eq!(dep.de_MTime.get(), 28 | 34 << 5 | 12 << 11);
    assert_eq!(dep.de_Attributes.get(), ATTR_ARCHIVE);
    assert_eq!(dep.de_ADate.get(), 0x21);
    assert_eq!(dep.de_CDate.get(), 0x21);
    assert_eq!((dep.de_CTime.get(), dep.de_CTimeHundredth.get()), (0, 0));

    // Without Win95 there are no access and creation times.
    pmp.pm_flags.set(MSDOSFSMNT_NOWIN95 as u32);
    dep.set_flag(DE_ACCESS | DE_CREATE);
    detimes(&dep, &now, &now, &now);
    assert_eq!(dep.de_ADate.get(), 0x21);
    assert_eq!(dep.de_CDate.get(), 0x21);
    assert_eq!(dep.de_flag.get(), DE_MODIFIED);
}

#[test]
fn defid_overlays_fid() {
    let defid = Defid {
        defid_len: size_of::<Defid>() as u16,
        defid_pad: 0,
        defid_dirclust: 7,
        defid_dirofs: 96,
    };
    let mut fid = Fid::default();
    defid.to_fid(&mut fid);
    assert_eq!(fid.fid_len, 12);
    assert_eq!(Defid::from_fid(&fid), defid);
}
