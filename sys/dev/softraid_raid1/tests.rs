use super::*;

extern crate std;
use std::boxed::Box;
use std::string::ToString;
use std::vec::Vec;

use crate::dev::biovar::{BIOC_SDINVALID, BIOC_SDUNUSED};
use crate::dev::softraid::SR_META_BYTES;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::scsi::scsi_disk::ScsiRw10;
use crate::scsi::scsiconf::{_lto4b, SCSI_DATA_OUT, ScsiXfer};
use crate::sys::malloc::M_WAITOK;

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

/// A discipline with in-memory metadata and `n` chunks in the given states.
fn volume(states: &[i32]) -> &'static SrDiscipline {
    // SAFETY: `SrSoftc` is a `Softc`: all-zero bytes are a valid value of it.
    let sc: &'static SrSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed::<SrSoftc>() }));
    let sd = sr_malloc::<SrDiscipline>(M_WAITOK).unwrap();
    // SAFETY: a zeroed discipline (`SrZeroed`), never freed in the tests.
    let sd: &'static SrDiscipline = unsafe { sd.as_ref() };
    sd.sd_sc.set(sc);
    sd.sd_meta.set(Some(
        sr_malloc_size::<SrMetadata>(SR_META_BYTES, M_WAITOK).unwrap(),
    ));
    sd.sd_meta().ssdi().ssd_chunk_no.set(states.len() as u32);
    sd.sd_meta().ssdi().ssd_secsize.set(512);
    sd.sd_meta().ssdi().ssd_size.set(1000);
    sd.sd_meta().ssd_data_blkno.set(64);
    sd.sd_vol.sv_chunks_alloc(states.len(), M_WAITOK).unwrap();
    for (i, &state) in states.iter().enumerate() {
        // SAFETY: a zeroed chunk (`SrZeroed`), leaked.
        let c: &'static SrChunk = unsafe { sr_malloc::<SrChunk>(M_WAITOK).unwrap().as_ref() };
        c.src_meta.scm_status.set(state as u32);
        sd.sd_vol.set_sv_chunk(i, Some(c));
    }
    sd
}

fn chunk_states(sd: &SrDiscipline) -> Vec<i32> {
    (0..sd.sd_vol.sv_nchunks())
        .map(|i| sd.sd_vol.sv_chunk(i).src_meta.scm_status.get() as i32)
        .collect()
}

#[test]
fn chunk_transition_table() {
    // what each state may go to, as the C's nested switch lists it
    let allowed = |old: i32| -> Vec<i32> {
        match old {
            BIOC_SDONLINE => std::vec![BIOC_SDOFFLINE, BIOC_SDSCRUB],
            BIOC_SDOFFLINE => std::vec![BIOC_SDREBUILD, BIOC_SDHOTSPARE],
            BIOC_SDSCRUB => std::vec![BIOC_SDONLINE],
            BIOC_SDREBUILD => std::vec![BIOC_SDONLINE, BIOC_SDOFFLINE],
            BIOC_SDHOTSPARE => std::vec![BIOC_SDOFFLINE, BIOC_SDREBUILD],
            _ => Vec::new(),
        }
    };
    for &old in &SD_STATES {
        for &new in &SD_STATES {
            assert_eq!(
                raid1_chunk_transition_ok(old, new),
                allowed(old).contains(&new),
                "{old} -> {new}"
            );
        }
    }
}

#[test]
fn volume_state_from_chunks() {
    let of = |chunks: &[i32]| {
        let mut states = [0usize; SR_MAX_STATES];
        for &c in chunks {
            states[c as usize] += 1;
        }
        raid1_vol_state(&states, chunks.len())
    };
    assert_eq!(of(&[BIOC_SDONLINE, BIOC_SDONLINE]), Some(BIOC_SVONLINE));
    assert_eq!(of(&[BIOC_SDOFFLINE, BIOC_SDOFFLINE]), Some(BIOC_SVOFFLINE));
    assert_eq!(of(&[BIOC_SDOFFLINE, BIOC_SDHOTSPARE]), Some(BIOC_SVOFFLINE));
    assert_eq!(of(&[BIOC_SDONLINE, BIOC_SDOFFLINE]), Some(BIOC_SVDEGRADED));
    assert_eq!(of(&[BIOC_SDONLINE, BIOC_SDREBUILD]), Some(BIOC_SVREBUILD));
    assert_eq!(of(&[BIOC_SDSCRUB, BIOC_SDONLINE]), Some(BIOC_SVSCRUB));
    // scrub wins over rebuild, rebuild over offline
    assert_eq!(
        of(&[BIOC_SDSCRUB, BIOC_SDREBUILD, BIOC_SDONLINE]),
        Some(BIOC_SVSCRUB)
    );
    assert_eq!(
        of(&[BIOC_SDREBUILD, BIOC_SDOFFLINE, BIOC_SDONLINE]),
        Some(BIOC_SVREBUILD)
    );
    // online chunks and only unusual ones
    assert_eq!(of(&[BIOC_SDONLINE, BIOC_SDHOTSPARE]), None);
    assert_eq!(of(&[BIOC_SDONLINE, BIOC_SDUNUSED]), None);
}

#[test]
fn volume_transition_table() {
    let allowed = |old: i32| -> Vec<i32> {
        match old {
            BIOC_SVONLINE => std::vec![
                BIOC_SVONLINE,
                BIOC_SVOFFLINE,
                BIOC_SVDEGRADED,
                BIOC_SVREBUILD
            ],
            BIOC_SVDEGRADED => std::vec![BIOC_SVOFFLINE, BIOC_SVREBUILD, BIOC_SVDEGRADED],
            BIOC_SVBUILDING => std::vec![BIOC_SVONLINE, BIOC_SVOFFLINE, BIOC_SVBUILDING],
            BIOC_SVSCRUB => std::vec![BIOC_SVONLINE, BIOC_SVOFFLINE, BIOC_SVDEGRADED, BIOC_SVSCRUB],
            BIOC_SVREBUILD => std::vec![
                BIOC_SVONLINE,
                BIOC_SVOFFLINE,
                BIOC_SVDEGRADED,
                BIOC_SVREBUILD
            ],
            _ => Vec::new(),
        }
    };
    for old in 0..8 {
        for new in 0..8 {
            assert_eq!(
                raid1_vol_transition_ok(old, new),
                allowed(old).contains(&new),
                "{old} -> {new}"
            );
        }
    }
}

#[test]
fn set_vol_state_follows_the_chunks() {
    let _g = setup_real_memory();
    let sd = volume(&[BIOC_SDONLINE, BIOC_SDONLINE, BIOC_SDONLINE]);
    sd.sd_vol_status.set(BIOC_SVONLINE);
    sr_raid1_set_vol_state(sd);
    assert_eq!(sd.sd_vol_status.get(), BIOC_SVONLINE);

    sd.sd_vol
        .sv_chunk(1)
        .src_meta
        .scm_status
        .set(BIOC_SDOFFLINE as u32);
    sr_raid1_set_vol_state(sd);
    assert_eq!(sd.sd_vol_status.get(), BIOC_SVDEGRADED);

    sd.sd_vol
        .sv_chunk(1)
        .src_meta
        .scm_status
        .set(BIOC_SDREBUILD as u32);
    sr_raid1_set_vol_state(sd);
    assert_eq!(sd.sd_vol_status.get(), BIOC_SVREBUILD);

    for i in 0..3 {
        sd.sd_vol
            .sv_chunk(i)
            .src_meta
            .scm_status
            .set(BIOC_SDOFFLINE as u32);
    }
    sr_raid1_set_vol_state(sd);
    assert_eq!(sd.sd_vol_status.get(), BIOC_SVOFFLINE);
}

#[test]
fn set_chunk_state_runs_the_machine() {
    let _g = setup_real_memory();
    let sd = volume(&[BIOC_SDONLINE, BIOC_SDONLINE]);
    sd.sd_set_vol_state.set(Some(sr_raid1_set_vol_state));
    sd.sd_vol_status.set(BIOC_SVONLINE);

    // same state: nothing happens
    sr_raid1_set_chunk_state(sd, 0, BIOC_SDONLINE);
    assert_eq!(sd.sd_must_flush.get(), 0);

    sr_raid1_set_chunk_state(sd, 1, BIOC_SDOFFLINE);
    assert_eq!(chunk_states(sd), [BIOC_SDONLINE, BIOC_SDOFFLINE]);
    assert_eq!(sd.sd_vol_status.get(), BIOC_SVDEGRADED);
    assert_eq!(sd.sd_must_flush.get(), 1);

    // offline -> rebuild -> offline aborts the rebuild
    sr_raid1_set_chunk_state(sd, 1, BIOC_SDREBUILD);
    assert_eq!(sd.sd_vol_status.get(), BIOC_SVREBUILD);
    assert_eq!(sd.sd_reb_abort.get(), 0);
    sr_raid1_set_chunk_state(sd, 1, BIOC_SDOFFLINE);
    assert_eq!(sd.sd_reb_abort.get(), 1);
    assert_eq!(sd.sd_vol_status.get(), BIOC_SVDEGRADED);
}

#[test]
fn reads_interleave_and_skip_dead_chunks() {
    let counter = core::cell::Cell::new(0u32);
    let all_online = |_: usize| BIOC_SDONLINE;
    let picks: Vec<_> = (0..7)
        .map(|_| raid1_read_chunk(&counter, 3, all_online).unwrap())
        .collect();
    assert_eq!(picks, [0, 1, 2, 0, 1, 2, 0]);

    // chunk 1 is rebuilding, 2 scrubbing: reads alternate 0 and 2
    let status = |c: usize| [BIOC_SDONLINE, BIOC_SDREBUILD, BIOC_SDSCRUB][c];
    counter.set(0);
    let picks: Vec<_> = (0..4)
        .map(|_| raid1_read_chunk(&counter, 3, status).unwrap())
        .collect();
    assert_eq!(picks, [0, 2, 0, 2]);

    // every chunk dead (or hotspare): none, after `no_chunk + 1` looks
    let dead = |c: usize| [BIOC_SDOFFLINE, BIOC_SDHOTSPARE, BIOC_SDREBUILD][c];
    counter.set(0);
    assert_eq!(raid1_read_chunk(&counter, 3, dead), None);
    assert_eq!(counter.get(), 4);

    // a state a volume cannot have: no retry
    counter.set(0);
    assert_eq!(raid1_read_chunk(&counter, 2, |_| BIOC_SDUNUSED), None);
    assert_eq!(counter.get(), 1);

    // the counter wraps
    counter.set(u32::MAX);
    assert_eq!(raid1_read_chunk(&counter, 4, all_online), Some(3));
    assert_eq!(counter.get(), 0);
}

#[test]
fn writes_go_to_working_chunks() {
    assert_eq!(raid1_write_action(BIOC_SDONLINE), Raid1Write::Write);
    assert_eq!(raid1_write_action(BIOC_SDSCRUB), Raid1Write::Write);
    assert_eq!(raid1_write_action(BIOC_SDREBUILD), Raid1Write::Write);
    assert_eq!(raid1_write_action(BIOC_SDOFFLINE), Raid1Write::Skip);
    assert_eq!(raid1_write_action(BIOC_SDHOTSPARE), Raid1Write::Skip);
    assert_eq!(raid1_write_action(BIOC_SDUNUSED), Raid1Write::Bad);
    assert_eq!(raid1_write_action(BIOC_SDINVALID), Raid1Write::Bad);
}

#[test]
fn init_create_and_hooks() {
    let _g = setup_real_memory();
    let sd = volume(&[BIOC_SDONLINE, BIOC_SDONLINE, BIOC_SDONLINE]);
    // SAFETY: a plain C structure of integers and null pointers: all-zero bytes are a value.
    let mut bc: BiocCreateraid = unsafe { core::mem::zeroed() };
    sr_raid1_create(sd, &mut bc, 3, 777).unwrap();
    assert_eq!(sd.sd_meta().ssdi().ssd_size.get(), 777);
    assert_eq!(sd.sd_max_ccb_per_wu.get(), 3);

    let sd = volume(&[BIOC_SDONLINE, BIOC_SDONLINE]);
    sr_raid1_assemble(sd, &mut bc, 2, None).unwrap();
    assert_eq!(sd.sd_max_ccb_per_wu.get(), 2);

    sr_raid1_discipline_init(sd);
    assert_eq!(sd.sd_type.get(), SR_MD_RAID1);
    assert_eq!(sd.name().to_string(), "RAID 1");
    assert_eq!(
        sd.sd_capabilities.get(),
        SR_CAP_SYSTEM_DISK | SR_CAP_AUTO_ASSEMBLE | SR_CAP_REBUILD | SR_CAP_REDUNDANT
    );
    assert_eq!(sd.sd_max_wu.get(), SR_RAID1_NOWU);
    assert!(sd.sd_create.get().is_some());
    assert!(sd.sd_assemble.get().is_some());
    assert!(sd.sd_scsi_rw.get().is_some());
    assert!(sd.sd_scsi_wu_done.get().is_some());
    assert!(sd.sd_set_chunk_state.get().is_some());
    assert!(sd.sd_set_vol_state.get().is_some());
}

/// A work unit of `sd` (leaked) with a transfer of `flags`.
fn work_unit(sd: &'static SrDiscipline, flags: i32) -> &'static SrWorkunit {
    let xs: &'static ScsiXfer = Box::leak(Box::new(ScsiXfer::new()));
    xs.flags.set(flags);
    // SAFETY: a zeroed work unit (`SrZeroed`), leaked.
    let wu: &'static SrWorkunit = unsafe { sr_malloc::<SrWorkunit>(M_WAITOK).unwrap().as_ref() };
    wu.swu_dis.set(sd);
    wu.swu_xs.set(Some(xs));
    wu
}

#[test]
fn wu_done_succeeds_when_one_io_did() {
    let _g = setup_real_memory();
    let sd = volume(&[BIOC_SDONLINE, BIOC_SDONLINE]);
    let wu = work_unit(sd, 0);
    wu.swu_ios_succeeded.set(1);
    wu.swu_ios_failed.set(1);
    wu.xs().error.set(XS_DRIVER_STUFFUP);
    assert_eq!(sr_raid1_wu_done(wu), SR_WU_OK);
    assert_eq!(wu.xs().error.get(), XS_NOERROR);
}

#[test]
fn wu_done_gives_up_on_a_failed_write() {
    let _g = setup_real_memory();
    let sd = volume(&[BIOC_SDONLINE, BIOC_SDONLINE]);
    let wu = work_unit(sd, 0);
    wu.swu_ios_failed.set(2);
    assert_eq!(sr_raid1_wu_done(wu), SR_WU_FAILED);
    assert_eq!(wu.swu_state.get(), SR_WU_FAILED);
    assert_eq!(wu.xs().error.get(), XS_DRIVER_STUFFUP);
}

#[test]
fn wu_done_restarts_a_failed_read() {
    let _g = setup_real_memory();
    let sd = volume(&[BIOC_SDONLINE, BIOC_SDONLINE]);
    // a read whose restart `sd_scsi_rw` refuses fails for good; one it accepts is restarted
    fn refuse(_wu: &'static SrWorkunit) -> Result<(), Errno> {
        Err(Errno::EIO)
    }
    fn accept(_wu: &'static SrWorkunit) -> Result<(), Errno> {
        Ok(())
    }
    let wu = work_unit(sd, SCSI_DATA_IN);
    wu.swu_ios_failed.set(1);
    wu.swu_state.set(SR_WU_INPROGRESS);
    sd.sd_scsi_rw.set(Some(refuse));
    assert_eq!(sr_raid1_wu_done(wu), SR_WU_FAILED);
    assert_eq!(wu.xs().error.get(), XS_DRIVER_STUFFUP);

    let wu = work_unit(sd, SCSI_DATA_IN);
    wu.swu_ios_failed.set(1);
    sd.sd_scsi_rw.set(Some(accept));
    assert_eq!(sr_raid1_wu_done(wu), SR_WU_RESTART);
    assert_eq!(wu.swu_state.get(), SR_WU_RESTART);
}

/// A read (`SCSI_DATA_IN`) or write work unit of 512 bytes at block 16 of an online volume.
fn io_work_unit(sd: &'static SrDiscipline, flags: i32) -> &'static SrWorkunit {
    sd.sd_vol_status.set(BIOC_SVONLINE);
    let wu = work_unit(sd, flags);
    let xs = wu.xs();
    let data: &'static mut [u8] = Box::leak(std::vec![0u8; 512].into_boxed_slice());
    // SAFETY: a leaked buffer only this transfer uses.
    unsafe { xs.set_data(data.as_mut_ptr(), 512) };
    xs.cmdlen.set(10);
    xs.with_cmd::<ScsiRw10, _>(|c| _lto4b(16, &mut c.addr));
    wu
}

#[test]
fn rw_fails_on_an_offline_volume_or_a_dead_mirror() {
    let _g = setup_real_memory();
    let sd = volume(&[BIOC_SDOFFLINE, BIOC_SDOFFLINE]);
    // `sr_validate_io` refuses an offline volume
    let wu = io_work_unit(sd, SCSI_DATA_IN);
    sd.sd_vol_status.set(BIOC_SVOFFLINE);
    assert_eq!(sr_raid1_rw(wu), Err(Errno::EIO));

    // an online volume whose mirrors are all gone cannot be read
    let wu = io_work_unit(sd, SCSI_DATA_IN);
    assert_eq!(sr_raid1_rw(wu), Err(Errno::EIO));
    assert_eq!(wu.swu_io_count.get(), 0);

    // a write to a chunk in a state no volume has fails; no ccb was queued
    let sd = volume(&[BIOC_SDUNUSED, BIOC_SDONLINE]);
    let wu = io_work_unit(sd, SCSI_DATA_OUT);
    assert_eq!(sr_raid1_rw(wu), Err(Errno::EIO));
    assert_eq!(wu.swu_io_count.get(), 0);
}
