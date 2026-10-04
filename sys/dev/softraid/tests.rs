use super::*;

extern crate std;
use std::boxed::Box;
use std::vec::Vec;

use crate::kern::subr_pool::tests::setup_real_memory;

/// A zeroed softc (what `config_make_softc` gives `sr_attach`), leaked.
fn softc() -> &'static SrSoftc {
    // SAFETY: `SrSoftc` is a `Softc`: all-zero bytes are a valid value of it.
    Box::leak(Box::new(unsafe { core::mem::zeroed::<SrSoftc>() }))
}

/// A zeroed discipline of `sc` with in-memory metadata, as `sr_ioctl_createraid` makes one.
fn discipline(sc: &'static SrSoftc) -> &'static SrDiscipline {
    let sd = sr_malloc::<SrDiscipline>(M_WAITOK).unwrap();
    // SAFETY: a zeroed discipline (`SrZeroed`), never freed in the tests.
    let sd: &'static SrDiscipline = unsafe { sd.as_ref() };
    sd.sd_sc.set(sc);
    sd.sd_meta.set(Some(
        sr_malloc_size::<SrMetadata>(SR_META_BYTES, M_WAITOK).unwrap(),
    ));
    sd
}

/// A discipline with `nchunks` online chunks of 1000 blocks.
fn volume(nchunks: usize) -> &'static SrDiscipline {
    let sd = discipline(softc());
    sd.sd_meta().ssdi().ssd_chunk_no.set(nchunks as u32);
    sd.sd_vol.sv_chunks_alloc(nchunks, M_WAITOK).unwrap();
    for i in 0..nchunks {
        // SAFETY: a zeroed chunk (`SrZeroed`), leaked.
        let c: &'static SrChunk = unsafe { sr_malloc::<SrChunk>(M_WAITOK).unwrap().as_ref() };
        c.src_meta.scm_status.set(BIOC_SDONLINE as u32);
        c.src_size.set(1000);
        c.src_dev_mm.set(0x0400 + i as Dev);
        sd.sd_vol.set_sv_chunk(i, Some(c));
    }
    sd
}

#[test]
fn stripsize() {
    assert_eq!(sr_validate_stripsize(512), Some(9));
    assert_eq!(sr_validate_stripsize(64 * 1024), Some(16));
    assert_eq!(sr_validate_stripsize(0), None);
    assert_eq!(sr_validate_stripsize(1000), None);
    assert_eq!(sr_validate_stripsize(3 * 512), None);
}

#[test]
fn checksum_is_md5() {
    let sc = softc();
    let abc: Vec<Cell<u8>> = b"abc".iter().map(|&b| Cell::new(b)).collect();
    assert_eq!(
        sr_checksum(sc, &abc),
        [
            0x90, 0x01, 0x50, 0x98, 0x3c, 0xd2, 0x4f, 0xb0, 0xd6, 0x96, 0x3f, 0x7d, 0x28, 0xe1,
            0x7f, 0x72
        ]
    );
    // longer than the 64-byte staging buffer
    let long: Vec<Cell<u8>> = (0..1000).map(|i| Cell::new(i as u8)).collect();
    let mut ctx = Md5Ctx::default();
    let mut want = [0u8; MD5_DIGEST_LENGTH];
    let bytes: Vec<u8> = (0..1000).map(|i| i as u8).collect();
    MD5Init(&mut ctx);
    MD5Update(&mut ctx, &bytes);
    MD5Final(&mut want, &mut ctx);
    assert_eq!(sr_checksum(sc, &long), want);
}

#[test]
fn uuid_version_and_format() {
    let mut u = SrUuid::default();
    sr_uuid_generate(&mut u);
    assert_eq!(u.sui_id[6] & 0xf0, 0x40);
    assert_eq!(u.sui_id[8] & 0xc0, 0x80);
    let u = SrUuid {
        sui_id: core::array::from_fn(|i| i as u8),
    };
    let s = sr_uuid_format(&u);
    assert_eq!(&s[..36], b"00010203-0405-0607-0809-0a0b0c0d0e0f");
    assert_eq!(s[36], 0);
}

#[test]
fn rebuild_percent() {
    let _g = setup_real_memory();
    let sd = volume(2);
    sd.sd_meta().ssdi().ssd_size.set(1000);
    assert_eq!(sr_rebuild_percent(sd), 0);
    sd.sd_meta().ssd_rebuild.set(500);
    assert_eq!(sr_rebuild_percent(sd), 49);
}

#[test]
fn chunk_in_use() {
    let _g = setup_real_memory();
    let sd = volume(2);
    let sc = sd.sd_sc();
    // SAFETY: a new discipline in no list, leaked.
    unsafe { sc.sc_dis_list.insert_tail(sd) };
    assert_eq!(sr_chunk_in_use(sc, 0x0401), BIOC_SDONLINE);
    assert_eq!(sr_chunk_in_use(sc, 0x0499), BIOC_SDINVALID);
    assert_eq!(sr_chunk_in_use(sc, NODEV), BIOC_SDINVALID);
}

#[test]
fn work_units_and_ccbs() {
    let _g = setup_real_memory();
    let sd = volume(2);
    sd.set_wu_type_default();
    sd.sd_max_wu.set(3);
    sd.sd_max_ccb_per_wu.set(2);
    sr_alloc_resources(sd).unwrap();
    assert_eq!(sd.sd_wu_pending.get(), 0);
    assert_eq!(sd.sd_wu.iter().count(), 3);
    assert_eq!(sd.sd_wu_freeq.iter().count(), 3);
    assert_eq!(sd.sd_ccb_freeq.iter().count(), 6);
    assert!(sr_ccb_alloc(sd).is_err()); // already there

    // SAFETY: the discipline's pool, with the discipline as cookie and its own get/put.
    unsafe {
        scsi_iopool_init(
            &sd.sd_iopool,
            ptr::from_ref(sd).cast_mut().cast(),
            sr_wu_get,
            sr_wu_put,
        )
    };
    let wu = sr_scsi_wu_get(sd, 0).unwrap();
    assert_eq!(sd.sd_wu_pending.get(), 1);
    assert!(ptr::eq(wu.dis(), sd));

    // ccbs: get, enqueue, account, release
    let ccbs: Vec<&'static SrCcb> = (0..6).map(|_| sr_ccb_get(sd).unwrap()).collect();
    assert!(sr_ccb_get(sd).is_none());
    for ccb in &ccbs[..2] {
        sr_wu_enqueue_ccb(wu, ccb);
    }
    for ccb in &ccbs[2..] {
        sr_ccb_put(ccb);
    }
    assert_eq!(wu.swu_io_count.get(), 2);
    assert!(ptr::eq(ccbs[0].wu(), wu));
    // SAFETY: `ccb_buf` is the first member of a ccb.
    assert!(ptr::eq(
        unsafe { sr_ccb_from_buf(&ccbs[1].ccb_buf) },
        ccbs[1]
    ));
    sr_wu_release_ccbs(wu);
    assert_eq!(wu.swu_io_count.get(), 0);
    assert_eq!(sd.sd_ccb_freeq.iter().count(), 6);
    assert_eq!(ccbs[0].ccb_target.get(), -1);

    sr_scsi_wu_put(sd, wu);
    assert_eq!(sd.sd_wu_pending.get(), 0);

    sr_free_resources(sd);
    assert!(sd.sd_wu.is_empty());
    assert!(sd.sd_ccb_freeq.is_empty());
    assert!(sd.sd_ccb.get().is_none());
}

#[repr(C)]
struct BigWu {
    wu: SrWorkunit,
    extra: Cell<u64>,
}
// SAFETY: a work unit and an integer cell, valid as zero; no `Drop`.
unsafe impl SrZeroed for BigWu {}
// SAFETY: `#[repr(C)]` with the work unit first.
unsafe impl SrWorkunitExt for BigWu {}

#[test]
fn extended_work_units() {
    let _g = setup_real_memory();
    let sd = volume(1);
    sd.set_wu_type::<BigWu>();
    sd.sd_max_wu.set(2);
    sd.sd_max_ccb_per_wu.set(1);
    sr_wu_alloc(sd).unwrap();
    let wu = sd.sd_wu.first().unwrap();
    // SAFETY: work units live until `sr_wu_free`.
    let wu: &'static SrWorkunit = unsafe { &*ptr::from_ref(wu) };
    let big = sr_wu_ext::<BigWu>(wu);
    big.extra.set(7);
    assert!(ptr::eq(&big.wu, wu));
    assert_eq!(sd.sd_wu_size(), size_of::<BigWu>());
    sr_wu_free(sd);
}

#[test]
fn schedule_defers_colliding_work_units() {
    let _g = setup_real_memory();
    let sd = volume(1);
    sd.set_wu_type_default();
    sd.sd_max_wu.set(2);
    sr_wu_alloc(sd).unwrap();
    let mut it = sd.sd_wu.iter();
    // SAFETY: work units live until `sr_wu_free`.
    let (a, b): (&'static SrWorkunit, &'static SrWorkunit) = unsafe {
        (
            &*ptr::from_ref(it.next().unwrap()),
            &*ptr::from_ref(it.next().unwrap()),
        )
    };
    // `a` is pending on blocks 0..9
    a.swu_blk_start.set(0);
    a.swu_blk_end.set(9);
    // SAFETY: `a` is on no processing queue once taken off the free queue.
    unsafe {
        sd.sd_wu_freeq.remove(a);
        sd.sd_wu_pendq.insert_tail(a);
        sd.sd_wu_freeq.remove(b);
    }
    // `b` overlaps it: deferred behind it
    b.swu_state.set(SR_WU_INPROGRESS);
    b.swu_io_count.set(1);
    b.swu_blk_start.set(5);
    b.swu_blk_end.set(12);
    sr_schedule_wu(b);
    assert_eq!(b.swu_state.get(), SR_WU_DEFERRED);
    assert!(ptr::eq(a.swu_collider.get().unwrap(), b));
    assert!(ptr::eq(sd.sd_wu_defq.first().unwrap(), b));
    assert_eq!(sd.sd_wu_collisions.get(), 1);

    // a work unit under construction is not scheduled
    b.swu_state.set(SR_WU_CONSTRUCT);
    sr_schedule_wu(b);
    assert_eq!(sd.sd_wu_defq.iter().count(), 1);
}

#[test]
fn chunk_and_volume_state() {
    let _g = setup_real_memory();
    let sd = volume(2);
    sd.sd_set_vol_state.set(Some(sr_set_vol_state));
    sd.sd_vol_status.set(BIOC_SVONLINE);
    sr_set_vol_state(sd);
    assert_eq!(sd.sd_vol_status.get(), BIOC_SVONLINE);
    sd.sd_vol
        .sv_chunk(1)
        .src_meta
        .scm_status
        .set(BIOC_SDOFFLINE as u32);
    sr_set_vol_state(sd);
    assert_eq!(sd.sd_vol_status.get(), BIOC_SVOFFLINE);
}

#[test]
fn meta_init_fills_volume_and_chunks() {
    let _g = setup_real_memory();
    let sd = volume(0);
    let cl = &sd.sd_vol.sv_chunk_list;
    let mut prev: Option<&'static SrChunk> = None;
    for (size, secsize) in [(1000, 512), (800, 4096), (1200, 512)] {
        // SAFETY: a zeroed chunk (`SrZeroed`), leaked.
        let c: &'static SrChunk = unsafe { sr_malloc::<SrChunk>(M_WAITOK).unwrap().as_ref() };
        c.src_size.set(size);
        c.src_secsize.set(secsize);
        sr_strlcpy_cell(&c.src_devname, b"sd0d");
        // SAFETY: new chunks in no list, leaked.
        unsafe {
            match prev {
                None => cl.insert_head(c),
                Some(p) => SrChunkHead::insert_after(p, c),
            }
        }
        prev = Some(c);
    }
    sd.sd_name.set(*b"RAID 1\0\0\0\0");
    sr_meta_init(sd, 1, 3);
    sr_meta_init_complete(sd);
    let m = sd.sd_meta();
    assert_eq!(m.ssdi().ssd_magic.get(), SR_MAGIC);
    assert_eq!(m.ssdi().ssd_chunk_no.get(), 3);
    assert_eq!(m.ssdi().ssd_level.get(), 1);
    assert_eq!(m.ssdi().ssd_secsize.get(), 4096);
    assert_eq!(m.ssd_data_blkno.get(), SR_DATA_OFFSET as u32);
    assert_eq!(sd.sd_vol.sv_chunk_minsz.get(), 800);
    assert_eq!(sd.sd_vol.sv_chunk_maxsz.get(), 1200);
    assert_eq!(&m.ssdi().ssd_vendor.get(), b"OPENBSD\0");
    assert_eq!(&m.ssdi().ssd_product.get()[..7], b"SR RAID");
    assert_eq!(&m.ssdi().ssd_revision.get(), b"006\0");
    let sc = sd.sd_sc();
    for (i, c) in cl.iter().enumerate() {
        let scm = &c.src_meta;
        assert_eq!(scm.scmi().scm_chunk_id.get(), i as u32);
        assert_eq!(scm.scmi().scm_coerced_size.get(), 800);
        assert_eq!(scm.scmi().scm_uuid.get(), m.ssdi().ssd_uuid.get());
        assert_eq!(
            scm.scm_checksum.get(),
            sr_checksum(sc, &scm.cells()[..MD5_DIGEST_LENGTH])
        );
    }
}

/// A metadata area with one chunk and one optional item of `som` (a header plus payload).
fn area_with_opt(som: &[u8]) -> SrMetaBuf {
    let m = SrMetaBuf::new(SR_META_BYTES, M_WAITOK).unwrap();
    m.md().ssdi().ssd_chunk_no.set(1);
    m.md().ssdi().ssd_opt_no.set(1);
    let off = size_of::<SrMetadata>() + size_of::<SrMetaChunk>();
    cells_write(&m.cells()[off..], som);
    m
}

#[test]
fn opt_load_variable_length() {
    let _g = setup_real_memory();
    let sc = softc();
    let len = size_of::<SrMetaBoot>();
    let tmp = SrMetaBuf::new(len, M_WAITOK).unwrap();
    let boot = SrMetaBoot::view(tmp.cells()).unwrap();
    boot.sbm_hdr.som_type.set(SR_OPT_BOOT);
    boot.sbm_hdr.som_length.set(len as u32);
    boot.sbm_root_duid.set(*b"rootduid");
    boot.sbm_hdr.som_checksum.set(sr_checksum(sc, tmp.cells()));
    let mut bytes = std::vec![0u8; len];
    cells_read(&mut bytes, tmp.cells());

    let area = area_with_opt(&bytes);
    let head = SrMetaOptHead::new();
    sr_meta_opt_load(sc, area.cells(), &head);
    let omi = head.first().unwrap();
    assert_eq!(omi.omi_som().som_type.get(), SR_OPT_BOOT);
    assert_eq!(
        omi.som_as::<SrMetaBoot>().unwrap().sbm_root_duid.get(),
        *b"rootduid"
    );
}

#[test]
fn opt_load_old_fixed_length() {
    let _g = setup_real_memory();
    let sc = softc();
    // old format: som_length 0, payload at SR_OLD_META_OPT_OFFSET, MD5 at the end
    let mut old = std::vec![0u8; SR_OLD_META_OPT_SIZE];
    old[..4].copy_from_slice(&SR_OPT_KEYDISK.to_ne_bytes());
    old[SR_OLD_META_OPT_OFFSET..SR_OLD_META_OPT_OFFSET + 4].copy_from_slice(b"mask");
    let cells: Vec<Cell<u8>> = old.iter().map(|&b| Cell::new(b)).collect();
    let sum = sr_checksum(sc, &cells[..SR_OLD_META_OPT_MD5]);
    old[SR_OLD_META_OPT_MD5..].copy_from_slice(&sum);

    let area = area_with_opt(&old);
    let head = SrMetaOptHead::new();
    sr_meta_opt_load(sc, area.cells(), &head);
    let omi = head.first().unwrap();
    assert_eq!(omi.omi_som().som_type.get(), SR_OPT_KEYDISK);
    assert_eq!(
        omi.omi_som().som_length.get() as usize,
        size_of::<SrMetaKeydisk>()
    );
    let kd = omi.som_as::<SrMetaKeydisk>().unwrap();
    assert_eq!(&kd.skm_maskkey.get()[..4], b"mask");
}

#[test]
fn meta_validate_versions() {
    let _g = setup_real_memory();
    let sd = volume(1);
    let sc = sd.sd_sc();
    let m = SrMetaBuf::new(SR_META_BYTES, M_WAITOK).unwrap();
    let md = m.md();
    md.ssdi().ssd_magic.set(SR_MAGIC);
    md.ssdi().ssd_version.set(4);
    md.ssd_checksum.set(sr_checksum(sc, md.ssdi().cells()));
    sr_meta_validate(sd, NODEV, md, ptr::null_mut()).unwrap();
    assert_eq!(md.ssdi().ssd_version.get(), SR_META_VERSION);
    assert_eq!(md.ssd_data_blkno.get(), SR_DATA_OFFSET as u32);
    assert_eq!(md.ssdi().ssd_secsize.get(), DEV_BSIZE as u32);
    assert_eq!(&md.ssdi().ssd_revision.get(), b"006\0");
}

#[test]
fn validate_io_decodes_cdbs() {
    let _g = setup_real_memory();
    let sd = volume(1);
    sd.sd_meta().ssd_data_blkno.set(SR_DATA_OFFSET as u32);
    sd.sd_meta().ssdi().ssd_secsize.set(512);
    sd.sd_meta().ssdi().ssd_size.set(100);
    sd.sd_vol_status.set(BIOC_SVONLINE);
    let xs: &'static ScsiXfer = Box::leak(Box::new(ScsiXfer::new()));
    let data: &'static mut [u8] = Box::leak(std::vec![0u8; 4096].into_boxed_slice());
    // SAFETY: a leaked buffer only this transfer uses.
    unsafe { xs.set_data(data.as_mut_ptr(), 4096) };
    xs.cmdlen.set(10);
    xs.with_cmd::<ScsiRw10, _>(|c| _lto4b(16, &mut c.addr));
    // SAFETY: a zeroed work unit (`SrZeroed`), leaked.
    let wu: &'static SrWorkunit = unsafe { sr_malloc::<SrWorkunit>(M_WAITOK).unwrap().as_ref() };
    wu.swu_dis.set(sd);
    wu.swu_xs.set(Some(xs));
    assert_eq!(sr_validate_io(wu, "test"), Ok(16));
    assert_eq!((wu.swu_blk_start.get(), wu.swu_blk_end.get()), (16, 23));

    // out of bounds: ILLEGAL REQUEST sense
    xs.with_cmd::<ScsiRw10, _>(|c| _lto4b(95, &mut c.addr));
    assert!(sr_validate_io(wu, "test").is_err());
    assert_eq!(sd.sd_scsi_sense.get().flags, SKEY_ILLEGAL_REQUEST);
    assert_eq!(sd.sd_scsi_sense.get().add_sense_code, 0x21);

    // offline volume
    sd.sd_vol_status.set(BIOC_SVOFFLINE);
    assert!(sr_validate_io(wu, "test").is_err());
}

/// Links `sd` into its softc's discipline list and names it.
fn attach_volume(sd: &'static SrDiscipline, name: &[u8], level: u32) {
    let m = sd.sd_meta();
    sr_strlcpy_cell(&m.ssd_devname, name);
    m.ssdi().ssd_level.set(level);
    m.ssdi().ssd_size.set(2048);
    sr_strlcpy_cell(&m.ssdi().ssd_vendor, b"OPENBSD");
    sd.sd_vol_status.set(BIOC_SVONLINE);
    // SAFETY: a new discipline on no list, leaked.
    unsafe { sd.sd_sc().sc_dis_list.insert_tail(sd) };
}

#[test]
fn bio_inquiries_report_volumes_disks_and_hotspares() {
    let _g = setup_real_memory();
    let sd = volume(2);
    let sc = sd.sd_sc();
    attach_volume(sd, b"sd5", 1);
    for i in 0..2 {
        let c = sd.sd_vol.sv_chunk(i);
        c.src_meta.scmi().scm_size.set(1000);
        sr_strlcpy_cell(&c.src_meta.scmi().scm_devname, b"sd0a");
    }
    // a hotspare after the volume
    // SAFETY: a zeroed chunk (`SrZeroed`), leaked.
    let hs: &'static SrChunk = unsafe { sr_malloc::<SrChunk>(M_WAITOK).unwrap().as_ref() };
    hs.src_meta.scm_status.set(BIOC_SDHOTSPARE as u32);
    sr_strlcpy_cell(&hs.src_meta.scmi().scm_devname, b"sd3a");
    sr_hotspare_list_append(sc, hs);
    sc.sc_hotspare_no.set(1);

    // SAFETY: the ioctl structures are valid as zero bytes.
    let mut bi: BiocInq = unsafe { core::mem::zeroed() };
    sr_ioctl_inq(sc, &mut bi).unwrap();
    assert_eq!((bi.bi_novol, bi.bi_nodisk), (2, 3));

    // SAFETY: as above.
    let mut bv: BiocVol = unsafe { core::mem::zeroed() };
    sr_ioctl_vol(sc, &mut bv).unwrap();
    assert_eq!((bv.bv_level, bv.bv_nodisk, bv.bv_size), (1, 2, 2048 << 9));
    assert_eq!(&bv.bv_dev[..4], b"sd5\0");
    assert_eq!(&bv.bv_vendor[..8], b"OPENBSD\0");
    bv.bv_volid = 1;
    sr_ioctl_vol(sc, &mut bv).unwrap();
    assert_eq!((bv.bv_level, bv.bv_nodisk), (-1, 1));
    bv.bv_volid = 2;
    assert_eq!(sr_ioctl_vol(sc, &mut bv), Err(Errno::EINVAL));

    // SAFETY: as above.
    let mut bd: BiocDisk = unsafe { core::mem::zeroed() };
    bd.bd_diskid = 1;
    sr_ioctl_disk(sc, &mut bd).unwrap();
    assert_eq!(
        (bd.bd_status, bd.bd_size, bd.bd_target),
        (BIOC_SDONLINE, 1000 << 9, 1)
    );
    assert_eq!(&bd.bd_vendor[..5], b"sd0a\0");
    bd.bd_diskid = 2; // no key disk on a RAID 1
    assert_eq!(sr_ioctl_disk(sc, &mut bd), Err(Errno::EINVAL));
    bd.bd_volid = 1;
    bd.bd_diskid = 0;
    sr_ioctl_disk(sc, &mut bd).unwrap();
    assert_eq!(bd.bd_status, BIOC_SDHOTSPARE);

    assert!(ptr::eq(sr_find_discipline(sc, b"sd5\0\0").unwrap(), sd));
    assert!(sr_find_discipline(sc, b"sd6").is_none());
    assert!(sr_already_assembled(sd));
}

#[test]
fn bio_arguments_round_trip_through_bytes() {
    // SAFETY: the ioctl structures are valid as zero bytes.
    let bv: BiocVol = unsafe { core::mem::zeroed() };
    let mut bytes = std::vec![0u8; size_of::<BiocVol>()];
    bio_put(
        &mut bytes,
        offset_of!(BiocVol, bv_volid),
        &3i32.to_ne_bytes(),
    );
    let mut bs = bv.bv_bio.bio_status;
    bs.bs_status = BIO_STATUS_ERROR;
    bs.bs_msg_count = 1;
    bs.bs_msgs[0].bm_type = BIO_MSG_WARN;
    bs.bs_msgs[0].bm_msg[..2].copy_from_slice(b"hi");
    bio_put_status(&mut bytes, &bs);
    let back: BiocVol = bio_arg(&bytes).unwrap();
    assert_eq!(back.bv_volid, 3);
    assert_eq!(back.bv_bio.bio_status.bs_status, BIO_STATUS_ERROR);
    assert_eq!(back.bv_bio.bio_status.bs_msgs[0].bm_type, BIO_MSG_WARN);
    assert_eq!(&back.bv_bio.bio_status.bs_msgs[0].bm_msg[..2], b"hi");
    assert_eq!(bio_arg::<BiocVol>(&bytes[1..]).err(), Some(Errno::EINVAL));
}

#[test]
fn sensors_follow_the_volume_state() {
    let _g = setup_real_memory();
    let sd = volume(1);
    let sc = sd.sd_sc();
    attach_volume(sd, b"sd5", 1);
    for (state, value, status) in [
        (BIOC_SVOFFLINE, SENSOR_DRIVE_FAIL, SENSOR_S_CRIT),
        (BIOC_SVDEGRADED, SENSOR_DRIVE_PFAIL, SENSOR_S_WARN),
        (BIOC_SVREBUILD, SENSOR_DRIVE_REBUILD, SENSOR_S_WARN),
        (BIOC_SVONLINE, SENSOR_DRIVE_ONLINE, SENSOR_S_OK),
        (BIOC_SVINVALID, 0, SENSOR_S_UNKNOWN),
    ] {
        sd.sd_vol_status.set(state);
        sr_sensors_refresh(ptr::from_ref(sc).cast_mut().cast());
        assert_eq!(sd.sd_vol.sv_sensor.value.get(), value);
        assert_eq!(sd.sd_vol.sv_sensor.status.get(), status);
    }
}

#[test]
fn discipline_free_releases_and_unlinks() {
    let _g = setup_real_memory();
    let sd = volume(2);
    let sc = sd.sd_sc();
    attach_volume(sd, b"sd5", 1);
    let omi = SrMetaOptItem::alloc(size_of::<SrMetaBoot>(), M_WAITOK).unwrap();
    // SAFETY: a new item in no list.
    unsafe { sd.sd_meta_opt.insert_head(omi) };
    sd.sd_target.set(7);
    sc.sc_targets[7].set(Some(sd));
    sd.mds()
        .mdd_crypto
        .scr_maskkey
        .set([0xa5; SR_CRYPTO_MAXKEYBYTES]);
    // chunks without vnodes are just freed
    let cl = &sd.sd_vol.sv_chunk_list;
    for i in 0..2 {
        // SAFETY: the volume's chunks are on no list yet.
        unsafe { cl.insert_head(sd.sd_vol.sv_chunk(i)) };
    }
    sr_chunks_unwind(sc, cl);
    assert!(cl.is_empty());
    sr_discipline_free(Some(sd));
    assert!(sc.sc_dis_list.is_empty());
    assert!(sc.sc_targets[7].get().is_none());
}
