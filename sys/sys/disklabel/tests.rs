//! Host tests for `<sys/disklabel.h>`: the device number split, the `DL_*` accessors, the
//! label's byte layout and the constants (against the C header).

use super::*;

#[test]
fn device_numbers_split_into_unit_and_partition() {
    // rd0a, rd0c and sd1c on amd64's majors.
    let rd0a = makediskdev(17, 0, 0);
    assert_eq!(major(rd0a), 17);
    assert_eq!(diskunit(rd0a), 0);
    assert_eq!(diskpart(rd0a), 0);
    let sd1c = makediskdev(4, 1, RAW_PART);
    assert_eq!(diskunit(sd1c), 1);
    assert_eq!(diskpart(sd1c), 2);
    assert_eq!(minor(sd1c), 66);
    assert_eq!(disklabeldev(makediskdev(4, 1, 5)), sd1c);
}

#[test]
fn split_fields_round_trip() {
    let mut p = Partition::new();
    dl_setpsize(&mut p, 0x1_2345_6789);
    dl_setpoffset(&mut p, 0x2_0000_0001);
    assert_eq!((p.p_sizeh, p.p_size), (1, 0x2345_6789));
    assert_eq!(dl_getpsize(&p), 0x1_2345_6789);
    assert_eq!(dl_getpoffset(&p), 0x2_0000_0001);

    let mut d = Disklabel::zeroed();
    d.d_secsize = 2048;
    dl_setdsize(&mut d, 0x3_0000_0000);
    dl_setbstart(&mut d, 64);
    dl_setbend(&mut d, 0x1_0000_0000);
    assert_eq!(dl_getdsize(&d), 0x3_0000_0000);
    assert_eq!(dl_getbstart(&d), 64);
    assert_eq!(dl_getbend(&d), 0x1_0000_0000);
    assert_eq!(dl_blkspersec(&d), 4);
    assert_eq!(dl_sectoblk(&d, 3), 12);
    assert_eq!(dl_blktosec(&d, 13), 3);
    assert_eq!(dl_blkoffset(&d, 13), 512);
}

#[test]
fn partition_names() {
    assert_eq!(dl_partnum2name(0), Some(b'a'));
    assert_eq!(dl_partnum2name(15), Some(b'p'));
    assert_eq!(dl_partnum2name(MAXPARTITIONS), None);
    assert_eq!(dl_partname2num(b'c'), Some(2));
    assert_eq!(dl_partname2num(b'z'), None); // beyond MAXPARTITIONS (16)
    assert_eq!(dl_partname2num(b'?'), None);
}

#[test]
fn ffs_fragblock_encoding() {
    // newfs's default: 16 KiB blocks of 8 fragments of 2 KiB.
    let fb = disklabelv1_ffs_fragblock(2048, 8);
    assert_eq!(disklabelv1_ffs_bsize(fb), 16384);
    assert_eq!(disklabelv1_ffs_frag(fb), 8);
    assert_eq!(disklabelv1_ffs_fsize(fb), 2048);
    assert_eq!(disklabelv1_ffs_fragblock(0, 8), 0);
}

#[test]
fn label_bytes_follow_the_c_layout() {
    let mut d = Disklabel::zeroed();
    d.d_magic = DISKMAGIC;
    d.d_secsize = 512;
    d.d_npartitions = 3;
    d.d_partitions[0].p_fstype = FS_BSDFFS;
    let b = d.as_bytes();
    assert_eq!(&b[0..4], &DISKMAGIC.to_ne_bytes());
    assert_eq!(&b[40..44], &512u32.to_ne_bytes());
    assert_eq!(&b[138..140], &3u16.to_ne_bytes());
    assert_eq!(b[148 + 12], FS_BSDFFS);
    assert_eq!(
        Disklabel::from_bytes(&b[..512]).d_partitions[0].p_fstype,
        FS_BSDFFS
    );
    // The checksum range ends after the third partition.
    assert_eq!(d.cksum_words(3).count(), (148 + 3 * 16) / 2);
}

#[test]
fn dos_table_reads_little_endian_entries() {
    let mut s = [0u8; 512];
    s[DOSPARTOFF + 16 + 4] = DOSPTYP_OPENBSD;
    s[DOSPARTOFF + 16 + 8..DOSPARTOFF + 16 + 12].copy_from_slice(&64u32.to_le_bytes());
    s[DOSPARTOFF + 16 + 12..DOSPARTOFF + 16 + 16].copy_from_slice(&1000u32.to_le_bytes());
    let dp = DosPartition::table(&s);
    assert_eq!(dp[1].dp_typ, DOSPTYP_OPENBSD);
    assert_eq!(u32::from_le(dp[1].dp_start), 64);
    assert_eq!(u32::from_le(dp[1].dp_size), 1000);
    assert_eq!(dp[0], DosPartition::default());
}

#[test]
fn partinfo_travels_through_an_ioctl_buffer() {
    let mut d = Disklabel::zeroed();
    let pi = Partinfo {
        disklab: &mut d,
        part: core::ptr::null_mut(),
    };
    let mut buf = [0u8; 16];
    pi.store(&mut buf);
    let back = Partinfo::load(&buf).expect("long enough");
    assert_eq!(back.disklab, pi.disklab);
    assert!(back.part.is_null());
    assert!(Partinfo::load(&buf[..8]).is_none());
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/sys/disklabel.h");
    let ours: &[(&str, i64)] = &[
        ("MAXPARTITIONSUNIT", MAXPARTITIONSUNIT.into()),
        ("MAXPARTITIONS16", MAXPARTITIONS16 as i64),
        ("RAW_PART", RAW_PART.into()),
        ("DISKMAGIC", DISKMAGIC.into()),
        ("NDDATA", NDDATA as i64),
        ("NSPARE", NSPARE as i64),
        ("DTYPE_SCSI", DTYPE_SCSI.into()),
        ("DTYPE_VND", DTYPE_VND.into()),
        ("DTYPE_RDROOT", DTYPE_RDROOT.into()),
        ("FS_UNUSED", FS_UNUSED.into()),
        ("FS_SWAP", FS_SWAP.into()),
        ("FS_BSDFFS", FS_BSDFFS.into()),
        ("FS_MSDOS", FS_MSDOS.into()),
        ("FS_OTHER", FS_OTHER.into()),
        ("FS_EXT2FS", FS_EXT2FS.into()),
        ("FS_NTFS", FS_NTFS.into()),
        ("FS_UDF", FS_UDF.into()),
        ("D_VENDOR", D_VENDOR.into()),
        ("GPTSECTOR", GPTSECTOR as i64),
        ("GPTREVISION", GPTREVISION.into()),
        ("NGPTPARTITIONS", NGPTPARTITIONS.into()),
        ("GPTMINHDRSIZE", GPTMINHDRSIZE.into()),
        ("GPTMINPARTSIZE", GPTMINPARTSIZE.into()),
        ("GPTPARTNAMESIZE", GPTPARTNAMESIZE as i64),
        ("DOS_LABELSECTOR", DOS_LABELSECTOR),
        ("DOSBBSECTOR", DOSBBSECTOR as i64),
        ("DOSPARTOFF", DOSPARTOFF as i64),
        ("DOSDISKOFF", DOSDISKOFF as i64),
        ("NDOSPART", NDOSPART as i64),
        ("DOSACTIVE", DOSACTIVE.into()),
        ("DOSMBR_SIGNATURE", DOSMBR_SIGNATURE.into()),
        ("DOSMBR_SIGNATURE_OFF", DOSMBR_SIGNATURE_OFF as i64),
        ("DOS_MAXEBR", DOS_MAXEBR.into()),
        ("DOSPTYP_UNUSED", DOSPTYP_UNUSED.into()),
        ("DOSPTYP_EXTEND", DOSPTYP_EXTEND.into()),
        ("DOSPTYP_EXTENDL", DOSPTYP_EXTENDL.into()),
        ("DOSPTYP_LINUX", DOSPTYP_LINUX.into()),
        ("DOSPTYP_OPENBSD", DOSPTYP_OPENBSD.into()),
        ("DOSPTYP_EFI", DOSPTYP_EFI.into()),
        ("DOSPTYP_EFISYS", DOSPTYP_EFISYS.into()),
    ];
    for (name, value) in ours {
        assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
    }
}
