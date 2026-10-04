use super::*;

extern crate std;
use std::boxed::Box;
use std::string::ToString;
use std::vec::Vec;

use crate::dev::softraid::SR_META_BYTES;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::malloc::M_WAITOK;

const STRIP: i64 = MAXPHYS as i64;
const STRIP_BITS: i64 = 16;

/// A zeroed discipline with in-memory metadata, as `sr_ioctl_createraid` makes one.
fn discipline() -> &'static SrDiscipline {
    // SAFETY: `SrSoftc` is a `Softc`: all-zero bytes are a valid value of it.
    let sc: &'static SrSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed::<SrSoftc>() }));
    let sd = sr_malloc::<SrDiscipline>(M_WAITOK).unwrap();
    // SAFETY: a zeroed discipline (`SrZeroed`), never freed in the tests.
    let sd: &'static SrDiscipline = unsafe { sd.as_ref() };
    sd.sd_sc.set(sc);
    sd.sd_meta.set(Some(
        sr_malloc_size::<SrMetadata>(SR_META_BYTES, M_WAITOK).unwrap(),
    ));
    sd
}

/// `(chunk, byte offset on the chunk)` of byte `pos` of a RAID 0 volume of `n` chunks, the
/// textbook way: strip `pos / STRIP` goes to chunk `strip % n`, as that chunk's strip
/// `strip / n`.
fn model(pos: i64, n: i64) -> (i64, i64) {
    let strip = pos / STRIP;
    (strip % n, (strip / n) * STRIP + pos % STRIP)
}

/// Every piece of the walk of `datalen` bytes at block `blkno` over `n` chunks, as
/// `(chunk, blkno on the chunk, length)`.
fn pieces(blkno: Daddr, datalen: i64, n: i64) -> Vec<(i64, Daddr, i64)> {
    let mut walk = Raid0Walk::new(blkno, datalen, STRIP, STRIP_BITS, n);
    let mut v = Vec::new();
    let mut count = 0;
    loop {
        v.push((walk.chunk, walk.blkno(), walk.length));
        count += 1;
        if !walk.advance(count == 1) {
            break;
        }
    }
    v
}

#[test]
fn volume_size_is_whole_strips_times_chunks() {
    // 64 KiB strip = 128 blocks
    assert_eq!(raid0_volume_size(1000, MAXPHYS as u32, 2), 896 * 2);
    assert_eq!(raid0_volume_size(1024, MAXPHYS as u32, 3), 1024 * 3);
    assert_eq!(raid0_volume_size(127, MAXPHYS as u32, 4), 0);
    assert_eq!(raid0_volume_size(1 << 40, MAXPHYS as u32, 2), 2 << 40);
}

#[test]
fn walk_within_one_strip() {
    // block 3 of the first strip: chunk 0, 100 bytes
    let p = pieces(3, 512, 2);
    assert_eq!(p, [(0, 3, 512)]);
    // the whole first strip of the volume
    assert_eq!(pieces(0, STRIP, 3), [(0, 0, STRIP)]);
    // the second strip lives on chunk 1, at its start
    assert_eq!(pieces(128, 4096, 3), [(1, 0, 4096)]);
    // strip 3 of 3 chunks wraps to chunk 0, second row
    assert_eq!(pieces(3 * 128, 512, 3), [(0, 128, 512)]);
}

#[test]
fn walk_crossing_strips() {
    // starts 1 block before the end of strip 0, runs into strips 1 and 2 (2 chunks): the
    // third piece wraps to chunk 0's second row
    let p = pieces(127, 512 + STRIP + 1024, 2);
    assert_eq!(p, [(0, 127, 512), (1, 0, STRIP), (0, 128, 1024)]);
    // an exactly aligned transfer over every chunk and into the next row
    let p = pieces(0, 5 * STRIP, 2);
    assert_eq!(
        p,
        [
            (0, 0, STRIP),
            (1, 0, STRIP),
            (0, 128, STRIP),
            (1, 128, STRIP),
            (0, 256, STRIP)
        ]
    );
    // a mid-strip start that wraps after its first piece (the `first` branch does not apply)
    let p = pieces(128 + 64, STRIP, 2);
    assert_eq!(p, [(1, 64, STRIP - 64 * 512), (0, 128, 64 * 512)]);
}

#[test]
fn walk_matches_the_textbook_layout() {
    for n in 2..=5i64 {
        for &blkno in &[0, 1, 63, 127, 128, 129, 500, 128 * 7 + 5] {
            for &datalen in &[512, 1024, 65536, 65536 + 512, 3 * 65536, 7 * 65536 + 1536] {
                let start = blkno * 512;
                let mut pos = start;
                let mut total = 0;
                for (chunk, cblk, length) in pieces(blkno, datalen, n) {
                    let (mchunk, moff) = model(pos, n);
                    assert_eq!(
                        (chunk, cblk * 512),
                        (mchunk, moff),
                        "n {n} blk {blkno} len {datalen}"
                    );
                    assert!(length > 0 && length <= STRIP);
                    // a piece never crosses a strip
                    assert_eq!(model(pos + length - 1, n).0, mchunk);
                    pos += length;
                    total += length;
                }
                assert_eq!(total, datalen);
            }
        }
    }
}

#[test]
fn init_sets_strip_bits_and_ccb_budget() {
    let _g = setup_real_memory();
    let sd = discipline();
    let ssdi = sd.sd_meta().ssdi();
    ssdi.ssd_strip_size.set(MAXPHYS as u32);
    ssdi.ssd_chunk_no.set(3);
    sr_raid0_init(sd).unwrap();
    assert_eq!(sd.mds().mdd_raid0.sr0_strip_bits.get(), 16);
    // (MAXPHYS / strip + 1) * SR_RAID0_NOWU * chunks
    assert_eq!(sd.sd_max_ccb_per_wu.get(), 2 * SR_RAID0_NOWU * 3);

    ssdi.ssd_strip_size.set(32 * 1024);
    ssdi.ssd_chunk_no.set(2);
    sr_raid0_init(sd).unwrap();
    assert_eq!(sd.mds().mdd_raid0.sr0_strip_bits.get(), 15);
    assert_eq!(sd.sd_max_ccb_per_wu.get(), 3 * SR_RAID0_NOWU * 2);
}

#[test]
fn discipline_init_installs_hooks() {
    let _g = setup_real_memory();
    let sd = discipline();
    sr_raid0_discipline_init(sd);
    assert_eq!(sd.sd_type.get(), SR_MD_RAID0);
    assert_eq!(sd.name().to_string(), "RAID 0");
    assert_eq!(
        sd.sd_capabilities.get(),
        SR_CAP_SYSTEM_DISK | SR_CAP_AUTO_ASSEMBLE
    );
    assert_eq!(sd.sd_max_wu.get(), SR_RAID0_NOWU);
    assert!(sd.sd_create.get().is_some());
    assert!(sd.sd_assemble.get().is_some());
    assert!(sd.sd_scsi_rw.get().is_some());
    assert!(sd.sd_scsi_sync.get().is_none());
}
