use super::*;

/// IDENTIFY data with the given 16-bit words set.
fn identify(words: &[(usize, u16)]) -> AtaIdentify {
    let mut bytes = [0u8; 512];
    for &(w, v) in words {
        bytes[2 * w..2 * w + 2].copy_from_slice(&v.to_le_bytes());
    }
    // SAFETY: `AtaIdentify` is 512 bytes of integers and byte arrays: any bytes are a value.
    unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<AtaIdentify>()) }
}

/// Writes an ATA string (byte-swapped words) at word `w`.
fn put_string(id: &mut AtaIdentify, w: usize, s: &[u8]) {
    let mut swapped = std::vec![0u8; s.len()];
    ata_swapcopy(s, &mut swapped);
    let mut bytes = *id.as_bytes();
    bytes[2 * w..2 * w + s.len()].copy_from_slice(&swapped);
    // SAFETY: as in `identify`.
    *id = unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<AtaIdentify>()) };
}

#[test]
fn swapcopy_swaps_word_bytes() {
    let mut dst = [0u8; 6];
    ata_swapcopy(b"EQUM  ", &mut dst);
    assert_eq!(&dst, b"QEMU  ");
    let mut short = [0u8; 3];
    ata_swapcopy(b"abcd", &mut short);
    assert_eq!(&short, b"ba\0");
}

#[test]
fn identify_capacity() {
    // LBA28: words 60-61; the C returns the last LBA.
    let id = identify(&[(60, 0x0000), (61, 0x0002)]);
    assert_eq!(ata_identify_blocks(&id), 0x2_0000 - 1);
    // LBA48 (word 83 bit 10): words 100-103.
    let id = identify(&[(83, 0x0400), (100, 0x5678), (101, 0x1234), (102, 1)]);
    assert_eq!(ata_identify_blocks(&id), 0x1_1234_5678 - 1);
    assert_eq!(ata_identify_blocksize(&id), 512);
    assert_eq!(ata_identify_block_l2p_exp(&id), 0);
    assert_eq!(ata_identify_block_logical_align(&id), 0);
    // 4 KB logical sectors (2048 words), 8 logical per physical, aligned at 1.
    let id = identify(&[
        (106, 0x4000 | 0x2000 | 0x1000 | 3),
        (117, 2048),
        (209, 0x4001),
    ]);
    assert_eq!(ata_identify_blocksize(&id), 4096);
    assert_eq!(ata_identify_block_l2p_exp(&id), 3);
    assert_eq!(ata_identify_block_logical_align(&id), 1);
}

#[test]
fn read_capacity_16_data() {
    let id = identify(&[
        (60, 0x1000),
        (106, 0x4000 | 0x2000 | 3),
        (209, 0x4001),
        (69, 0x4000),
    ]);
    let rcd = atascsi_read_cap_data_16(&id, ATA_PORT_F_TRIM);
    assert_eq!(_8btol(&rcd.addr), 0xfff);
    assert_eq!(_4btol(&rcd.length), 512);
    assert_eq!(rcd.logical_per_phys, 3);
    // (1 << 3) - 1 = 7, with TPE and TPRZ.
    assert_eq!(
        _2btol(&rcd.lowest_aligned) as u16,
        7 | READ_CAP_16_TPE | READ_CAP_16_TPRZ
    );
    let rcd = atascsi_read_cap_data_16(&identify(&[(60, 8)]), 0);
    assert_eq!(_2btol(&rcd.lowest_aligned), 0);
}

#[test]
fn rw_fis_lba28_lba48_and_ncq() {
    let mut fis = AtaFisH2d::default();
    atascsi_disk_rw_fis(&mut fis, ATA_H2D_FLAGS_CMD, false, false, 3, 0x0123_4567, 8);
    assert_eq!(fis.command, ATA_C_READDMA);
    assert_eq!(fis.device, ATA_H2D_DEVICE_LBA | 0x1);
    assert_eq!((fis.lba_low, fis.lba_mid, fis.lba_high), (0x67, 0x45, 0x23));
    assert_eq!(fis.sector_count, 8);

    let mut fis = AtaFisH2d::default();
    atascsi_disk_rw_fis(
        &mut fis,
        ATA_H2D_FLAGS_CMD,
        true,
        false,
        3,
        0x12_3456_789a,
        0x200,
    );
    assert_eq!(fis.command, ATA_C_WRITEDMA_EXT);
    assert_eq!(fis.device, ATA_H2D_DEVICE_LBA);
    assert_eq!(
        (fis.lba_low_exp, fis.lba_mid_exp, fis.lba_high_exp),
        (0x34, 0x12, 0x00)
    );
    assert_eq!((fis.sector_count, fis.sector_count_exp), (0x00, 0x02));

    // 0x100 sectors still fit LBA28 (count 0 means 256).
    let mut fis = AtaFisH2d::default();
    atascsi_disk_rw_fis(&mut fis, ATA_H2D_FLAGS_CMD, true, false, 0, 0x10, 0x100);
    assert_eq!((fis.command, fis.sector_count), (ATA_C_WRITEDMA, 0));

    let mut fis = AtaFisH2d::default();
    atascsi_disk_rw_fis(&mut fis, ATA_H2D_FLAGS_CMD | 2, false, true, 5, 0x10, 0x180);
    assert_eq!(fis.command, ATA_C_READ_FPDMA);
    assert_eq!(fis.flags, ATA_H2D_FLAGS_CMD | 2);
    assert_eq!(fis.sector_count, 5 << 3);
    assert_eq!((fis.features, fis.features_exp), (0x80, 0x01));
}

#[test]
fn inquiry_and_device_id() {
    let mut id = identify(&[]);
    put_string(&mut id, 27, b"QEMU HARDDISK                           ");
    put_string(&mut id, 23, b"2.5+    ");
    put_string(&mut id, 10, b"QM00001             ");

    let inq = atascsi_inquiry_data(&id);
    assert_eq!(inq.device, T_DIRECT);
    assert_eq!(&inq.vendor, b"ATA     ");
    assert_eq!(&inq.product, b"QEMU HARDDISK   ");
    assert_eq!(&inq.revision, b"2.5+");
    assert_ne!(inq.flags & SID_CmdQue, 0);

    // Without a WWN: a T10 identifier "ATA" + model + serial, 68 bytes.
    let (pg, len) = atascsi_vpd_ident_page(&id);
    assert_eq!(len, 4 + 4 + 68);
    assert_eq!(pg[1], SI_PG_DEVID);
    assert_eq!(_2btol(&pg[2..4]), 72);
    assert_eq!(pg[4], VPD_DEVID_CODE_ASCII);
    assert_eq!(pg[5], VPD_DEVID_ASSOC_LU | VPD_DEVID_TYPE_T10);
    assert_eq!(pg[7], 68);
    assert_eq!(&pg[8..16], b"ATA     ");
    assert_eq!(&pg[16..29], b"QEMU HARDDISK");
    assert_eq!(&pg[56..63], b"QM00001");

    // With one: the 8-byte NAA name of words 108-111.
    let id = identify(&[(87, ATA_ID_F87_WWN), (108, 0x5001), (109, 0x2345)]);
    let (pg, len) = atascsi_vpd_ident_page(&id);
    assert_eq!(len, 4 + 4 + 8);
    assert_eq!(pg[5], VPD_DEVID_ASSOC_LU | VPD_DEVID_TYPE_NAA);
    assert_eq!(&pg[8..12], &[0x50, 0x01, 0x23, 0x45]);
}

#[test]
fn atapi_sense_from_the_error_register() {
    // Sense key 5 (illegal request) in the high nibble, ABRT.
    let mut sd = ScsiSenseData::zeroed();
    atascsi_atapi_sense(&mut sd, 0x54);
    assert_eq!(sd.error_code, SSD_ERRCODE_CURRENT);
    assert_eq!(sd.flags, SKEY_ILLEGAL_REQUEST);
    // Sense key 3 with EOM and ILI.
    let mut sd = ScsiSenseData::zeroed();
    atascsi_atapi_sense(&mut sd, 0x33);
    assert_eq!(sd.flags, 0x3 | SSD_EOM | SSD_ILI);
}

#[test]
fn trim_descriptors_and_qdepth() {
    assert_eq!(ata_dsm_trim_desc(0x1234, 8), 0x0008_0000_0000_1234);
    assert_eq!(ata_qdepth(31), 32);
    assert_eq!(ata_qdepth(0xffff), 32);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/dev/ata/atascsi.h");
    let mut names = crate::reftest::assert_defines!(defs;
        ATA_C_READDMA_EXT, ATA_C_READ_LOG_EXT, ATA_C_WRITEDMA_EXT, ATA_C_READ_FPDMA,
        ATA_C_WRITE_FPDMA, ATA_C_PACKET, ATA_C_IDENTIFY_PACKET, ATA_C_READDMA, ATA_C_WRITEDMA,
        ATA_C_STANDBY_IMMED, ATA_C_READ_PM, ATA_C_WRITE_PM, ATA_C_FLUSH_CACHE,
        ATA_C_FLUSH_CACHE_EXT, ATA_C_IDENTIFY, ATA_C_SET_FEATURES, ATA_C_SEC_FREEZE_LOCK,
        ATA_C_DSM, ATA_SF_WRITECACHE_EN, ATA_SF_XFERMODE, ATA_SF_SATA_FEATURE_EN,
        ATA_SF_XFERMODE_UDMA, ATA_SF_SATA_FEATURE_DIS, ATA_SF_LOOKAHEAD_EN, ATA_SF_SATA_DEVIPS,
        ATA_SF_SATA_DEVAPS, ATA_SF_SATA_DEVSLEEP, ATA_ID_VALIDINFO_ULTRADMA,
        ATA_ID_ADD_SUPPORT_DRT, ATA_SATACAP_GEN1, ATA_SATACAP_GEN2, ATA_SATACAP_GEN3,
        ATA_SATACAP_NCQ, ATA_SATACAP_HIPM, ATA_SATACAP_HOSTAPS, ATA_SATACAP_DEVAPS,
        ATA_SATAFSUP_DIPM, ATA_SATAFSUP_DEVSLP, ATA_SATAFEN_DIPM, ATA_SATAFEN_DEVSLP,
        ATA_ID_F87_WWN, ATA_ID_P2L_SECT_MASK, ATA_ID_P2L_SECT_VALID, ATA_ID_P2L_SECT_SET,
        ATA_ID_P2L_SECT_SIZESET, ATA_ID_P2L_SECT_SIZE, ATA_ID_FORM_MASK,
        ATA_ID_DATA_SET_MGMT_TRIM, ATA_ID_LALIGN_MASK, ATA_ID_LALIGN_VALID, ATA_ID_LALIGN,
        ATA_IDENTIFY_WRITECACHE, ATA_IDENTIFY_LOOKAHEAD, ATA_DSM_TRIM, ATA_DSM_TRIM_MAX_LEN,
        ATA_FIS_LENGTH, ATA_FIS_TYPE_H2D, ATA_H2D_FLAGS_CMD, ATA_H2D_FEATURES_DMA,
        ATA_H2D_FEATURES_DIR, ATA_H2D_FEATURES_DIR_READ, ATA_H2D_FEATURES_DIR_WRITE,
        ATA_H2D_DEVICE_LBA, ATA_FIS_CONTROL_SRST, ATA_FIS_CONTROL_4BIT, ATA_FIS_TYPE_D2H,
        ATA_D2H_FLAGS_INTR, ATA_LOG_10H_TYPE_NOTQUEUED, ATA_LOG_10H_TYPE_TAG_MASK,
        SATA_SStatus_DET, SATA_SStatus_DET_NODEV, SATA_SStatus_DET_NOPHY,
        SATA_SStatus_DET_DEV, SATA_SStatus_DET_OFFLINE, SATA_SStatus_SPD,
        SATA_SStatus_SPD_NONE, SATA_SStatus_SPD_1_5, SATA_SStatus_SPD_3_0,
        SATA_SStatus_SPD_6_0, SATA_SStatus_IPM, SATA_SStatus_IPM_NODEV,
        SATA_SStatus_IPM_ACTIVE, SATA_SStatus_IPM_PARTIAL, SATA_SStatus_IPM_SLUMBER,
        SATA_SStatus_IPM_DEVSLEEP, SATA_SIGNATURE_PORT_MULTIPLIER, SATA_SIGNATURE_ATAPI,
        SATA_SIGNATURE_DISK, ATA_F_READ, ATA_F_WRITE, ATA_F_NOWAIT, ATA_F_POLL, ATA_F_PIO,
        ATA_F_PACKET, ATA_F_NCQ, ATA_F_DONE, ATA_F_GET_RFIS, ATA_S_SETUP, ATA_S_PENDING,
        ATA_S_COMPLETE, ATA_S_ERROR, ATA_S_TIMEOUT, ATA_S_ONCHIP, ATA_S_PUT, ATA_S_DONE,
        ASAA_CAP_NCQ, ASAA_CAP_NEEDS_RESERVED, ASAA_CAP_PMP_NCQ, ATA_PORT_T_NONE,
        ATA_PORT_T_DISK, ATA_PORT_T_ATAPI, ATA_PORT_T_PM,
    );
    // The `%b` string and the include guard are not simple defines.
    names.extend(["ATA_FMT_FLAGS", "_DEV_ATA_ATASCSI_H_"]);
    crate::reftest::assert_complete(&defs, "ATA_", &names);
    crate::reftest::assert_complete(&defs, "SATA_", &names);
    crate::reftest::assert_complete(&defs, "ASAA_", &names);
    assert_eq!(
        ATA_FMT_FLAGS,
        b"\x10\x09GET_RFIS\x08DONE\x07NCQ\x06PACKET\x05PIO\x04POLL\x03NOWAIT\x02WRITE\x01READ"
    );

    let defs = crate::reftest::defines("sys/dev/ata/atascsi.c");
    crate::reftest::assert_defines!(defs; ATA_PORT_F_NCQ, ATA_PORT_F_TRIM);
}
