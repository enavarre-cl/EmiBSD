//! Host tests of `msdosfs_fat.rs`: the FAT entry packing of the three widths over a block's
//! bytes, the FAT block arithmetic, the fat cache, and the in-use bitmap searches.

use super::*;
use std::boxed::Box;
use std::vec;
use std::vec::Vec;

/// A mount of `maxcluster` clusters with the given FAT width and an in-use bitmap in which
/// every cluster is free except 0 and 1 and the bits past `maxcluster`.
fn mount(fatmask: u32, maxcluster: u32) -> &'static Msdosfsmount {
    let pmp: &'static Msdosfsmount = Box::leak(Box::new(Msdosfsmount::new()));
    pmp.pm_fatmask.set(fatmask);
    let (mult, div) = match fatmask {
        FAT12_MASK => (3, 2),
        FAT16_MASK => (2, 1),
        _ => (4, 1),
    };
    pmp.pm_fatmult.set(mult);
    pmp.pm_fatdiv.set(div);
    pmp.pm_maxcluster.set(maxcluster);
    let words = (maxcluster as usize + 1).div_ceil(32);
    let map: &'static mut [u32] = Box::leak(vec![0u32; words].into_boxed_slice());
    pmp.pm_inusemap.set(map.as_mut_ptr());
    for cn in 0..words as u32 * 32 {
        if cn < CLUST_FIRST || cn > maxcluster {
            let w = &pmp.inusemap()[(cn / 32) as usize];
            w.set(w.get() | 1 << (cn % 32));
        }
    }
    pmp.pm_freeclustercount.set(maxcluster - 1);
    pmp
}

/// The FAT bytes of `entries` (cluster 0 first) for a FAT of the given width, written through
/// `fat_write`, and checked back through `fat_read`.
fn pack(fatmask: u32, mult: u32, div: u32, entries: &[u32]) -> Vec<u8> {
    let mut fat = vec![0u8; entries.len() * 4 + 4];
    for (cn, &v) in entries.iter().enumerate() {
        let bo = (cn as u32 * mult / div) as usize;
        fat_write(fatmask, &mut fat, bo, cn as u32, v);
    }
    for (cn, &v) in entries.iter().enumerate() {
        let bo = (cn as u32 * mult / div) as usize;
        assert_eq!(
            fat_read(fatmask, &fat, bo, cn as u32),
            v & fatmask,
            "cluster {cn}"
        );
    }
    fat
}

#[test]
fn fat12_entries_share_nibbles() {
    // Clusters 0..3: media 0xff8, 0xfff, then 3 -> 0xfff (EOF), 0x123.
    let fat = pack(FAT12_MASK, 3, 2, &[0xff8, 0xfff, 0xfff, 0x123]);
    assert_eq!(&fat[..6], &[0xf8, 0xff, 0xff, 0xff, 0x3f, 0x12]);
    // Rewriting an odd entry leaves its even neighbour alone, and the reverse.
    let mut fat = fat;
    fat_write(FAT12_MASK, &mut fat, 4, 3, 0xabc);
    assert_eq!(fat_read(FAT12_MASK, &fat, 3, 2), 0xfff);
    assert_eq!(fat_read(FAT12_MASK, &fat, 4, 3), 0xabc);
    fat_write(FAT12_MASK, &mut fat, 3, 2, 0x005);
    assert_eq!(fat_read(FAT12_MASK, &fat, 4, 3), 0xabc);
    assert_eq!(&fat[3..6], &[0x05, 0xc0, 0xab]);
    // CLUST_EOFE is cut to 12 bits.
    fat_write(FAT12_MASK, &mut fat, 4, 3, CLUST_EOFE);
    assert_eq!(fat_read(FAT12_MASK, &fat, 4, 3), 0xfff);
    assert_eq!(fat_read(FAT12_MASK, &fat, 3, 2), 0x005);
}

#[test]
fn fat16_and_fat32_entries() {
    let fat = pack(FAT16_MASK, 2, 1, &[0xfff8, 0xffff, 0x0003, 0xffff]);
    assert_eq!(&fat[..8], &[0xf8, 0xff, 0xff, 0xff, 0x03, 0x00, 0xff, 0xff]);
    let mut fat = pack(FAT32_MASK, 4, 1, &[0x0fff_fff8, 0x0fff_ffff, 0x0000_0003]);
    assert_eq!(&fat[8..12], &[0x03, 0, 0, 0]);
    // FAT32 keeps the high four bits of an entry.
    fat[11] = 0xa0;
    fat_write(FAT32_MASK, &mut fat, 8, 2, CLUST_EOFE);
    assert_eq!(&fat[8..12], &[0xff, 0xff, 0xff, 0xaf]);
    assert_eq!(fat_read(FAT32_MASK, &fat, 8, 2), FAT32_MASK);
    // An unknown width writes nothing.
    fat_write(0, &mut fat, 8, 2, 0);
    assert_eq!(&fat[8..12], &[0xff, 0xff, 0xff, 0xaf]);
}

#[test]
fn fat_blocks() {
    let pmp = mount(FAT12_MASK, 4000);
    // 512-byte sectors: FAT12 blocks of 3 sectors, a 12-sector FAT at block 1, two copies.
    pmp.pm_fatblocksize.set(3 * 512);
    pmp.pm_fatblocksec.set(3);
    pmp.pm_FATsecs.set(12);
    pmp.pm_fatblk.set(1);
    assert_eq!(fatblock(pmp, fatofs(pmp, 0)), (1, 1536, 0));
    // Cluster 1025 is at byte 1537: the second block, offset 1.
    assert_eq!(fatblock(pmp, fatofs(pmp, 1025)), (4, 1536, 1));
    // The last block is cut at the end of the FAT.
    pmp.pm_FATsecs.set(10);
    assert_eq!(fatblock(pmp, 5000), (10, 512, 5000 - 3 * 1536));
    // The current FAT of a FAT32 file system that does not mirror.
    pmp.pm_curfat.set(1);
    assert_eq!(fatblock(pmp, 0), (11, 1536, 0));
}

#[test]
fn fat_cache() {
    let dep = Denode::new();
    fc_purge(&dep, 0);
    let (mut i, mut cn) = (0, 77);
    fc_lookup(&dep, 10, &mut i, &mut cn);
    assert_eq!((i, cn), (0, 77), "an empty cache leaves the start");
    fc_setcache(&dep, FC_LASTMAP, 4, 104);
    fc_setcache(&dep, FC_LASTFC, 9, 109);
    fc_setcache(&dep, FC_OLASTFC, 6, 106);
    fc_lookup(&dep, 8, &mut i, &mut cn);
    assert_eq!((i, cn), (6, 106));
    fc_lookup(&dep, 100, &mut i, &mut cn);
    assert_eq!((i, cn), (9, 109));
    fc_purge(&dep, 6);
    assert_eq!(dep.fc(FC_LASTMAP).fc_frcn, 4);
    assert_eq!(dep.fc(FC_LASTFC).fc_frcn, FCE_EMPTY);
    assert_eq!(dep.fc(FC_OLASTFC).fc_frcn, FCE_EMPTY);
    fc_lookup(&dep, 100, &mut i, &mut cn);
    assert_eq!((i, cn), (4, 104));
}

#[test]
fn usemap_and_chainlength() {
    let pmp = mount(FAT16_MASK, 100);
    assert_eq!(chainlength(pmp, 2, 10), 10);
    assert_eq!(chainlength(pmp, 101, 1), 0);
    assert_eq!(chainlength(pmp, 2, 1000), 99);
    usemap_alloc(pmp, 40);
    assert_eq!(pmp.pm_freeclustercount.get(), 98);
    assert_eq!(chainlength(pmp, 30, 20), 10);
    assert_eq!(chainlength(pmp, 40, 20), 0);
    assert_eq!(chainlength(pmp, 2, 100), 38);
    usemap_free(pmp, 40);
    assert_eq!(pmp.pm_freeclustercount.get(), 99);
    assert_eq!(chainlength(pmp, 30, 20), 20);
    // A run that starts in one word and ends in the next.
    usemap_alloc(pmp, 70);
    assert_eq!(chainlength(pmp, 60, 50), 10);
    // A run up to the last cluster stops at the in-use bits past it.
    assert_eq!(chainlength(pmp, 71, 100), 30);
}

#[test]
fn scanfree_finds_runs() {
    let pmp = mount(FAT16_MASK, 100);
    for cn in 2..50 {
        usemap_alloc(pmp, cn);
    }
    usemap_alloc(pmp, 60);
    let (mut foundcn, mut foundl) = (0, 0);
    // From 10: the first free cluster is 50, a run of 10.
    assert_eq!(
        scanfree(pmp, 10, 101, 5, &mut foundcn, &mut foundl),
        Some(50)
    );
    assert_eq!(
        scanfree(pmp, 10, 101, 20, &mut foundcn, &mut foundl),
        Some(61)
    );
    assert_eq!(scanfree(pmp, 10, 101, 50, &mut foundcn, &mut foundl), None);
    assert_eq!((foundcn, foundl), (61, 40));
    let (mut foundcn, mut foundl) = (0, 0);
    assert_eq!(scanfree(pmp, 0, 55, 50, &mut foundcn, &mut foundl), None);
    assert_eq!((foundcn, foundl), (50, 10));
}
