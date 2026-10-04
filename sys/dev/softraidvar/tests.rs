use super::*;

extern crate std;

#[test]
fn packed_cells_are_machine_order_and_unaligned() {
    let area: [Cell<u8>; 400] = core::array::from_fn(|_| Cell::new(0));
    // An `sr_meta_chunk` at an odd offset, as the second chunk of a metadata area is.
    let mc: &SrMetaChunk = sr_view(&area, 168 + 92).unwrap();
    mc.scmi().scm_size.set(0x0102_0304_0506_0708);
    mc.scm_status.set(7);
    let off = 168 + 92 + core::mem::offset_of!(SrMetaChunkInvariant, scm_size);
    let mut b = [0u8; 8];
    cells_read(&mut b, &area[off..off + 8]);
    assert_eq!(i64::from_ne_bytes(b), 0x0102_0304_0506_0708);
    assert_eq!(mc.scm_status.get(), 7);
    assert!(sr_view::<SrMetaChunk>(&area, 400 - 91).is_none());
}

#[test]
fn view_copy_and_bzero() {
    let a: [Cell<u8>; 168] = core::array::from_fn(|i| Cell::new(i as u8));
    let b: [Cell<u8>; 168] = core::array::from_fn(|_| Cell::new(0));
    let ma = SrMetadata::view(&a).unwrap();
    let mb = SrMetadata::view(&b).unwrap();
    mb.copy_from(ma);
    assert_eq!(mb.ssdi().ssd_magic.get(), ma.ssdi().ssd_magic.get());
    assert_eq!(b[167].get(), 167);
    mb.bzero();
    assert!(b.iter().all(|c| c.get() == 0));
    assert_eq!(mb.cells().len(), 168);
}

#[test]
fn kdfinfo_hint_aliases() {
    let mut k = SrCryptoKdfinfo::default();
    k._kdfhint.generic.r#type = SR_CRYPTOKDFT_BCRYPT_PBKDF;
    assert_eq!(k.genkdf().r#type, SR_CRYPTOKDFT_BCRYPT_PBKDF);
    assert_eq!(k.pbkdf().generic.r#type, SR_CRYPTOKDFT_BCRYPT_PBKDF);
}

#[test]
fn name_display_stops_at_nul() {
    let mut n = [0u8; 10];
    n[..6].copy_from_slice(b"RAID 1");
    assert_eq!(std::format!("{}", Name(n)), "RAID 1");
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/dev/softraidvar.h");
    let _ = crate::reftest::assert_defines!(defs;
        SR_META_VERSION, SR_META_SIZE, SR_META_OFFSET, SR_BOOT_OFFSET, SR_BOOT_LOADER_SIZE,
        SR_BOOT_LOADER_OFFSET, SR_BOOT_BLOCKS_SIZE, SR_BOOT_BLOCKS_OFFSET, SR_BOOT_SIZE,
        SR_CRYPTO_MAXKEYBYTES, SR_CRYPTO_MAXKEYS, SR_CRYPTO_KEYBITS, SR_CRYPTO_KDFHINTBYTES,
        SR_CRYPTO_CHECKBYTES, SR_CRYPTO_KEY_BLKSHIFT,
        SR_CRYPTOKDFT_INVALID, SR_CRYPTOKDFT_PKCS5_PBKDF2, SR_CRYPTOKDFT_KEYDISK,
        SR_CRYPTOKDFT_BCRYPT_PBKDF, SR_CRYPTOKDF_INVALID, SR_CRYPTOKDF_KEY, SR_CRYPTOKDF_HINT,
        SR_IOCTL_GET_KDFHINT, SR_IOCTL_CHANGE_PASSPHRASE,
        SR_META_V3_SIZE, SR_META_V3_OFFSET, SR_META_V3_DATA_OFFSET,
        SR_META_F_NATIVE, SR_META_F_INVALID, SR_HEADER_SIZE, SR_DATA_OFFSET,
        SR_HOTSPARE_LEVEL, SR_HOTSPARE_VOLID, SR_KEYDISK_LEVEL, SR_KEYDISK_VOLID, SR_UUID_MAX,
        SR_MAGIC, SR_META_DIRTY, SR_OPT_INVALID, SR_OPT_CRYPTO, SR_OPT_BOOT, SR_OPT_KEYDISK,
        SR_CRYPTOA_AES_XTS_128, SR_CRYPTOA_AES_XTS_256, SR_CRYPTOF_INVALID, SR_CRYPTOF_KEY,
        SR_CRYPTOF_KDFHINT, SR_CRYPTOM_AES_ECB_256, SR_CRYPTOC_HMAC_SHA1, SR_MAX_BOOT_DISKS,
        SR_OLD_META_OPT_SIZE, SR_OLD_META_OPT_OFFSET,
        SR_MAX_LD, SR_MAX_CMDS, SR_MAX_STATES, SR_VM_IGNORE_DIRTY, SR_REBUILD_IO_SIZE,
        SR_CCB_FREE, SR_CCB_INPROGRESS, SR_CCB_OK, SR_CCB_FAILED, SR_CCBF_FREEBUF,
        SR_WU_FREE, SR_WU_INPROGRESS, SR_WU_OK, SR_WU_FAILED, SR_WU_PARTIALLYFAILED,
        SR_WU_DEFERRED, SR_WU_PENDING, SR_WU_RESTART, SR_WU_REQUEUE, SR_WU_CONSTRUCT,
        SR_WUF_REBUILD, SR_WUF_REBUILDIOCOMP, SR_WUF_FAIL, SR_WUF_FAILIOCOMP, SR_WUF_WAKEUP,
        SR_WUF_DISCIPLINE, SR_WUF_FAKE,
        SR_RAID0_NOWU, SR_RAID1_NOWU, SR_RAID5_NOWU, SR_RAID6_NOWU, SR_CRYPTO_NOWU,
        SR_CONCAT_NOWU, SR_RAID1C_NOWU,
        SR_MD_RAID0, SR_MD_RAID1, SR_MD_RAID5, SR_MD_CACHE, SR_MD_CRYPTO, SR_MD_RAID6,
        SR_MD_CONCAT, SR_MD_RAID1C,
        SR_CAP_SYSTEM_DISK, SR_CAP_AUTO_ASSEMBLE, SR_CAP_REBUILD, SR_CAP_NON_COERCED,
        SR_CAP_REDUNDANT);
}
