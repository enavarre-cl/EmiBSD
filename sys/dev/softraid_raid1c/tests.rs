//! Tests of RAID 1C: the read interleave and the write rule over every chunk state, the
//! place-holders for missing chunks, and the hooks the discipline installs.

use super::*;

extern crate std;
use core::cell::Cell;
use std::boxed::Box;
use std::vec::Vec;

use crate::dev::biovar::{BIOC_SDINVALID, BIOC_SDUNUSED};
use crate::dev::softraid::SR_META_BYTES;
use crate::kern::subr_pool::tests::setup_real_memory;

const SD_STATES: [i32; 8] = [
    BIOC_SDONLINE,
    BIOC_SDOFFLINE,
    BIOC_SDINVALID,
    BIOC_SDREBUILD,
    BIOC_SDHOTSPARE,
    BIOC_SDUNUSED,
    BIOC_SDSCRUB,
    7,
];

/// A discipline with in-memory metadata of `chunk_no` chunks, the first `present` of them
/// online, in `sv_chunks` and on `sv_chunk_list` (leaked).
fn volume(chunk_no: usize, present: usize) -> &'static SrDiscipline {
    // SAFETY: `SrSoftc` is a `Softc`: all-zero bytes are a valid value of it.
    let sc: &'static SrSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed::<SrSoftc>() }));
    // SAFETY: a zeroed discipline (`SrZeroed`), never freed in the tests.
    let sd: &'static SrDiscipline =
        unsafe { sr_malloc::<SrDiscipline>(M_WAITOK).unwrap().as_ref() };
    sd.sd_sc.set(sc);
    sd.sd_meta.set(Some(
        sr_malloc_size::<SrMetadata>(SR_META_BYTES, M_WAITOK).unwrap(),
    ));
    sd.sd_meta().ssdi().ssd_chunk_no.set(chunk_no as u32);
    sd.sd_vol.sv_chunks_alloc(present, M_WAITOK).unwrap();
    sd.sd_vol.sv_chunk_list.init();
    let mut prev: Option<&'static SrChunk> = None;
    for i in 0..present {
        // SAFETY: a zeroed chunk (`SrZeroed`), leaked.
        let c: &'static SrChunk = unsafe { sr_malloc::<SrChunk>(M_WAITOK).unwrap().as_ref() };
        c.src_meta.scm_status.set(BIOC_SDONLINE as u32);
        c.src_dev_mm.set(0x0400 + i as i32);
        match prev {
            // SAFETY: new chunks on no other list; `p` is on this one.
            None => unsafe { sd.sd_vol.sv_chunk_list.insert_head(c) },
            Some(p) => unsafe { SlistHead::<SrChunkLink>::insert_after(p, c) },
        }
        prev = Some(c);
        sd.sd_vol.set_sv_chunk(i, Some(c));
    }
    sd
}

#[test]
fn reads_interleave_over_readable_chunks() {
    let counter = Cell::new(0u32);
    let all_online = |_| BIOC_SDONLINE;
    let picks: Vec<_> = (0..5)
        .map(|_| raid1c_read_chunk(&counter, 3, all_online))
        .collect();
    assert_eq!(picks, [Some(0), Some(1), Some(2), Some(0), Some(1)]);

    // offline, rebuilding and hotspare chunks are passed over
    for skipped in [BIOC_SDOFFLINE, BIOC_SDREBUILD, BIOC_SDHOTSPARE] {
        let counter = Cell::new(0u32);
        let st = |c: usize| if c == 0 { skipped } else { BIOC_SDSCRUB };
        assert_eq!(raid1c_read_chunk(&counter, 2, st), Some(1));
        assert_eq!(raid1c_read_chunk(&counter, 2, st), Some(1));
        assert_eq!(counter.get(), 4);
    }

    // with none readable, the volume is offline after no_chunk retries
    let counter = Cell::new(0u32);
    assert_eq!(raid1c_read_chunk(&counter, 3, |_| BIOC_SDOFFLINE), None);
    assert_eq!(counter.get(), 4);
    // an unknown state stops at once
    let counter = Cell::new(0u32);
    assert_eq!(raid1c_read_chunk(&counter, 3, |_| BIOC_SDINVALID), None);
    assert_eq!(counter.get(), 1);

    // the counter wraps
    let counter = Cell::new(u32::MAX);
    assert_eq!(raid1c_read_chunk(&counter, 2, all_online), Some(1));
    assert_eq!(counter.get(), 0);
}

#[test]
fn write_rule_over_every_state() {
    for &st in &SD_STATES {
        let want = match st {
            BIOC_SDONLINE | BIOC_SDSCRUB | BIOC_SDREBUILD => Raid1cWrite::Write,
            BIOC_SDHOTSPARE | BIOC_SDOFFLINE => Raid1cWrite::Skip,
            _ => Raid1cWrite::Bad,
        };
        assert_eq!(raid1c_write_action(st, false), want, "state {st}");
        // a rebuild writes only to the chunks that are not online
        let want_rebuild = if st == BIOC_SDONLINE {
            Raid1cWrite::Skip
        } else {
            want
        };
        assert_eq!(raid1c_write_action(st, true), want_rebuild, "state {st}");
    }
}

#[test]
fn missing_chunks_become_offline_place_holders() {
    let _g = setup_real_memory();
    let sd = volume(3, 1);
    let first = sd.sd_vol.sv_chunk(0);
    sr_raid1c_add_offline_chunks(sd, 1).unwrap();

    assert_eq!(sd.sd_vol.sv_nchunks(), 3);
    assert!(core::ptr::eq(sd.sd_vol.sv_chunk(0), first));
    let list: Vec<&SrChunk> = sd.sd_vol.sv_chunk_list.iter().collect();
    assert_eq!(list.len(), 3);
    for c in 1..3 {
        let ch = sd.sd_vol.sv_chunk(c);
        assert_eq!(ch.src_meta.scm_status.get(), BIOC_SDOFFLINE as u32);
        assert_eq!(ch.src_dev_mm.get(), NODEV);
        assert!(
            core::ptr::eq(list[c], ch),
            "linked in order after the last found"
        );
    }

    // nothing found, or more than the volume has, is refused
    let sd = volume(2, 0);
    assert_eq!(sr_raid1c_add_offline_chunks(sd, 0), Err(Errno::EINVAL));
    let sd = volume(2, 2);
    assert_eq!(sr_raid1c_add_offline_chunks(sd, 3), Err(Errno::EINVAL));
}

#[test]
fn discipline_init_combines_raid1_and_crypto() {
    let _g = setup_real_memory();
    let sd = volume(2, 2);
    sr_raid1c_discipline_init(sd);
    assert_eq!(sd.sd_type.get(), SR_MD_RAID1C);
    assert_eq!(&sd.sd_name.get()[..8], b"RAID 1C\0");
    assert_eq!(
        sd.sd_capabilities.get(),
        SR_CAP_SYSTEM_DISK | SR_CAP_AUTO_ASSEMBLE | SR_CAP_REBUILD | SR_CAP_REDUNDANT
    );
    assert_eq!(sd.sd_max_wu.get(), SR_RAID1C_NOWU);
    assert_eq!(sd.sd_wu_size(), size_of::<SrCryptoWuReq>());
    assert!(
        sd.mds()
            .mdd_raid1c
            .sr1c_crypto
            .scr_sid
            .iter()
            .all(|s| s.get() == u64::MAX)
    );
    let f = |a: Option<SdScsiWuDoneFn>| a.map(|f| f as usize);
    assert_eq!(
        f(sd.sd_scsi_wu_done.get()),
        f(Some(sr_raid1_wu_done as SdScsiWuDoneFn))
    );
    assert!(sd.sd_set_chunk_state.get().is_some() && sd.sd_set_vol_state.get().is_some());
    assert!(sd.sd_scsi_rw.get().is_some() && sd.sd_scsi_done.get().is_some());
    assert!(sd.sd_meta_opt_handler.get().is_some() && sd.sd_ioctl_handler.get().is_some());
}

#[test]
fn meta_opt_handler_fills_the_crypto_half() {
    let _g = setup_real_memory();
    let sd = volume(2, 2);
    sr_raid1c_discipline_init(sd);
    let omi = SrMetaOptItem::alloc(size_of::<SrMetaCrypto>(), M_WAITOK).unwrap();
    omi.omi_som().som_type.set(SR_OPT_CRYPTO);
    sr_raid1c_meta_opt_handler(sd, omi).unwrap();
    assert!(sd.mds().mdd_raid1c.sr1c_crypto.scr_meta().is_some());
    assert!(sd.mds().mdd_crypto.scr_meta().is_none());
}
