/* $OpenBSD: softraid_raid1.c,v 1.67 2021/05/16 15:12:37 deraadt Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2007 Marco Peereboom <marco@peereboom.us>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

//! `softraid_raid1.c`: the RAID 1 (mirroring) discipline of softraid(4). Writes go to every
//! working chunk, reads are interleaved over the chunks that are online (or scrubbing), a
//! work unit succeeds when at least one of its I/Os did, and the chunk and volume state
//! machines (`sr_raid1_set_chunk_state`, `sr_raid1_set_vol_state`) decide when the volume is
//! degraded, rebuilding or offline. RAID 1C (`softraid_raid1c.c`) reuses `sr_raid1_init`,
//! `sr_raid1_assemble`, the two state functions and `sr_raid1_wu_done`.
//!
//! Upstream: sys/dev/softraid_raid1.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The chunk and volume state machines are split into pure functions of the states
//!   ([`raid1_chunk_transition_ok`], [`raid1_vol_state`], [`raid1_vol_transition_ok`]) that
//!   the host tests walk through; `sr_raid1_set_chunk_state` and `sr_raid1_set_vol_state`
//!   keep the C's locking, side effects and panics around them. Likewise the choice of the
//!   chunk to read from ([`raid1_read_chunk`]) and what a write does to a chunk
//!   ([`raid1_write_action`]).
//! - The C's `return (1)` is `Err(Errno::EIO)` (`softraidvar.rs`); `sr_raid1_create`'s
//!   `EINVAL` is kept; `sr_raid1_wu_done`'s `sd_scsi_rw(wu) == 0` is `is_ok()`.
//! - `sr1_counter` is a `u32` that wraps (the C's unsigned increment).
//! - `DNPRINTF` calls and the `SR_DEBUG` chunk status dump are comments (`SR_DEBUG` is not
//!   configured).

use crate::dev::biovar::{
    BIOC_SDHOTSPARE, BIOC_SDOFFLINE, BIOC_SDONLINE, BIOC_SDREBUILD, BIOC_SDSCRUB, BIOC_SVBUILDING,
    BIOC_SVDEGRADED, BIOC_SVOFFLINE, BIOC_SVONLINE, BIOC_SVREBUILD, BIOC_SVSCRUB, BiocCreateraid,
};
use crate::dev::softraid::{
    sr_ccb_rw, sr_error, sr_schedule_wu, sr_validate_io, sr_wu_enqueue_ccb, sr_wu_release_ccbs,
};
use crate::dev::softraidvar::*;
use crate::kern::kern_task::task_add;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::intr::{splbio, splx};
use crate::scsi::scsiconf::{SCSI_DATA_IN, XS_DRIVER_STUFFUP, XS_NOERROR};
use crate::sys::errno::Errno;
use crate::sys::task::SYSTQ;

/// `sr_raid1_discipline_init`: discipline initialisation.
pub fn sr_raid1_discipline_init(sd: &'static SrDiscipline) {
    // Fill out discipline members.
    sd.sd_type.set(SR_MD_RAID1);
    sd.sd_name.set(*b"RAID 1\0\0\0\0");
    sd.sd_capabilities
        .set(SR_CAP_SYSTEM_DISK | SR_CAP_AUTO_ASSEMBLE | SR_CAP_REBUILD | SR_CAP_REDUNDANT);
    sd.sd_max_wu.set(SR_RAID1_NOWU);

    // Setup discipline specific function pointers.
    sd.sd_assemble.set(Some(sr_raid1_assemble));
    sd.sd_create.set(Some(sr_raid1_create));
    sd.sd_scsi_rw.set(Some(sr_raid1_rw));
    sd.sd_scsi_wu_done.set(Some(sr_raid1_wu_done));
    sd.sd_set_chunk_state.set(Some(sr_raid1_set_chunk_state));
    sd.sd_set_vol_state.set(Some(sr_raid1_set_vol_state));
}

/// `sr_raid1_create`: sets up the metadata of a new RAID 1 volume of `no_chunk` chunks of
/// `coerced_size` blocks.
pub fn sr_raid1_create(
    sd: &'static SrDiscipline,
    _bc: &mut BiocCreateraid,
    no_chunk: i32,
    coerced_size: i64,
) -> Result<(), Errno> {
    if no_chunk < 2 {
        sr_error(
            sd.sd_sc(),
            format_args!("{} requires two or more chunks", sd.name()),
        );
        return Err(Errno::EINVAL);
    }

    sd.sd_meta().ssdi().ssd_size.set(coerced_size);

    sr_raid1_init(sd)
}

/// `sr_raid1_assemble`: brings up an existing RAID 1 volume.
pub fn sr_raid1_assemble(
    sd: &'static SrDiscipline,
    _bc: &mut BiocCreateraid,
    _no_chunk: i32,
    _data: Option<&[u8]>,
) -> Result<(), Errno> {
    sr_raid1_init(sd)
}

/// `sr_raid1_init`: initialises the runtime values (the ccb budget: one per chunk) from the
/// metadata.
pub fn sr_raid1_init(sd: &'static SrDiscipline) -> Result<(), Errno> {
    sd.sd_max_ccb_per_wu
        .set(sd.sd_meta().ssdi().ssd_chunk_no.get());

    Ok(())
}

/// Whether a chunk may go from `old_state` to `new_state` (`BIOC_SD*`); `old_state ==
/// new_state` is the caller's early exit. The nested `switch` of `sr_raid1_set_chunk_state`.
fn raid1_chunk_transition_ok(old_state: i32, new_state: i32) -> bool {
    match old_state {
        BIOC_SDONLINE => matches!(new_state, BIOC_SDOFFLINE | BIOC_SDSCRUB),
        BIOC_SDOFFLINE => matches!(new_state, BIOC_SDREBUILD | BIOC_SDHOTSPARE),
        BIOC_SDSCRUB => new_state == BIOC_SDONLINE,
        BIOC_SDREBUILD => matches!(new_state, BIOC_SDONLINE | BIOC_SDOFFLINE),
        BIOC_SDHOTSPARE => matches!(new_state, BIOC_SDOFFLINE | BIOC_SDREBUILD),
        _ => false,
    }
}

/// `sr_raid1_set_chunk_state`: moves chunk `c` to `new_state` (`BIOC_SD*`) if the state
/// machine allows it (else panics), updates the volume state and has the metadata saved.
pub fn sr_raid1_set_chunk_state(sd: &'static SrDiscipline, c: usize, new_state: i32) {
    // DNPRINTF(SR_D_STATE, "%s: %s: %s: sr_raid1_set_chunk_state %d -> %d")

    // ok to go to splbio since this only happens in error path
    let s = splbio();
    let chunk = sd.sd_vol.sv_chunk(c);
    let old_state = chunk.src_meta.scm_status.get() as i32;

    // multiple IOs to the same chunk that fail will come through here
    if old_state == new_state {
        splx(s);
        return;
    }

    if !raid1_chunk_transition_ok(old_state, new_state) {
        splx(s); // XXX
        panic(format_args!(
            "{}: {}: {}: invalid chunk state transition {} -> {}",
            DEVNAME(sd.sd_sc()),
            Name(sd.sd_meta().ssd_devname.get()),
            Name(chunk.src_meta.scmi().scm_devname.get()),
            old_state,
            new_state
        ));
    }

    if old_state == BIOC_SDREBUILD && new_state == BIOC_SDOFFLINE {
        // Abort rebuild since the rebuild chunk disappeared.
        sd.sd_reb_abort.set(1);
    }

    chunk.src_meta.scm_status.set(new_state as u32);
    sd.sd_set_vol_state();

    sd.sd_must_flush.set(1);
    let _ = task_add(SYSTQ, &sd.sd_meta_save_task);

    splx(s);
}

/// The volume state the chunk states `states` (the number of chunks in each `BIOC_SD*`
/// state) of an `nd`-chunk volume give; `None` for a combination that cannot happen.
fn raid1_vol_state(states: &[usize; SR_MAX_STATES], nd: usize) -> Option<i32> {
    if states[BIOC_SDONLINE as usize] == nd {
        Some(BIOC_SVONLINE)
    } else if states[BIOC_SDONLINE as usize] == 0 {
        Some(BIOC_SVOFFLINE)
    } else if states[BIOC_SDSCRUB as usize] != 0 {
        Some(BIOC_SVSCRUB)
    } else if states[BIOC_SDREBUILD as usize] != 0 {
        Some(BIOC_SVREBUILD)
    } else if states[BIOC_SDOFFLINE as usize] != 0 {
        Some(BIOC_SVDEGRADED)
    } else {
        None
    }
}

/// Whether the volume may go from `old_state` to `new_state` (`BIOC_SV*`; staying where it
/// is is allowed from every state but offline, which no state may leave, nor stay in). The
/// nested `switch` of `sr_raid1_set_vol_state`.
fn raid1_vol_transition_ok(old_state: i32, new_state: i32) -> bool {
    match old_state {
        // can go to same state; REBUILD happens on boot
        BIOC_SVONLINE => matches!(
            new_state,
            BIOC_SVONLINE | BIOC_SVOFFLINE | BIOC_SVDEGRADED | BIOC_SVREBUILD
        ),
        // XXX this might be a little too much
        BIOC_SVOFFLINE => false,
        BIOC_SVDEGRADED => matches!(new_state, BIOC_SVOFFLINE | BIOC_SVREBUILD | BIOC_SVDEGRADED),
        BIOC_SVBUILDING => matches!(new_state, BIOC_SVONLINE | BIOC_SVOFFLINE | BIOC_SVBUILDING),
        BIOC_SVSCRUB => matches!(
            new_state,
            BIOC_SVONLINE | BIOC_SVOFFLINE | BIOC_SVDEGRADED | BIOC_SVSCRUB
        ),
        BIOC_SVREBUILD => matches!(
            new_state,
            BIOC_SVONLINE | BIOC_SVOFFLINE | BIOC_SVDEGRADED | BIOC_SVREBUILD
        ),
        _ => false,
    }
}

/// `sr_raid1_set_vol_state`: recomputes the volume state from the chunks' (panics on a
/// transition the state machine forbids) and, when it has just become degraded, has a
/// hotspare looked for.
pub fn sr_raid1_set_vol_state(sd: &'static SrDiscipline) {
    let mut states = [0usize; SR_MAX_STATES];
    let old_state = sd.sd_vol_status.get();

    // DNPRINTF(SR_D_STATE, "%s: %s: sr_raid1_set_vol_state")

    let nd = sd.sd_meta().ssdi().ssd_chunk_no.get() as usize;

    // SR_DEBUG: DNPRINTF(SR_D_STATE, "%s: chunk %d status = %u") for every chunk

    for i in 0..nd {
        let chunk = sd.sd_vol.sv_chunk(i);
        let s = chunk.src_meta.scm_status.get() as usize;
        if s >= SR_MAX_STATES {
            panic(format_args!(
                "{}: {}: {}: invalid chunk state",
                DEVNAME(sd.sd_sc()),
                Name(sd.sd_meta().ssd_devname.get()),
                Name(chunk.src_meta.scmi().scm_devname.get())
            ));
        }
        states[s] += 1;
    }

    let Some(new_state) = raid1_vol_state(&states, nd) else {
        // DNPRINTF(SR_D_STATE, "%s: invalid volume state, old state was %d")
        panic(format_args!("invalid volume state"));
    };

    // DNPRINTF(SR_D_STATE, "%s: %s: sr_raid1_set_vol_state %d -> %d")

    if !raid1_vol_transition_ok(old_state, new_state) {
        panic(format_args!(
            "{}: {}: invalid volume state transition {} -> {}",
            DEVNAME(sd.sd_sc()),
            Name(sd.sd_meta().ssd_devname.get()),
            old_state,
            new_state
        ));
    }

    sd.sd_vol_status.set(new_state);

    // If we have just become degraded, look for a hotspare.
    if new_state == BIOC_SVDEGRADED {
        let _ = task_add(SYSTQ, &sd.sd_hotspare_rebuild_task);
    }
}

/// The chunk the next read goes to: the discipline's counter interleaves reads over the
/// chunks; one that is online or scrubbing is taken, offline, rebuilding and hotspare ones
/// are skipped (at most `no_chunk` times); `None` when there is none (volume offline) or a
/// chunk has a state that cannot be read. `status` gives the `BIOC_SD*` state of a chunk.
fn raid1_read_chunk(
    counter: &core::cell::Cell<u32>,
    no_chunk: u32,
    status: impl Fn(usize) -> i32,
) -> Option<usize> {
    let mut rt = 0;
    loop {
        // interleave reads
        let n = counter.get();
        counter.set(n.wrapping_add(1));
        let chunk = (n % no_chunk) as usize;
        match status(chunk) {
            BIOC_SDONLINE | BIOC_SDSCRUB => return Some(chunk),
            BIOC_SDOFFLINE | BIOC_SDREBUILD | BIOC_SDHOTSPARE => {
                let again = rt < no_chunk;
                rt += 1;
                if again {
                    continue;
                }
                // volume offline
                return None;
            }
            // volume offline
            _ => return None,
        }
    }
}

/// What a write does to a chunk in state `status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Raid1Write {
    /// The chunk is working: write to it.
    Write,
    /// The chunk is out: skip it (`continue`).
    Skip,
    /// A state no volume should be in: fail the work unit.
    Bad,
}

/// What a write does to a chunk in state `status` (`BIOC_SD*`): writes go on all working
/// disks.
fn raid1_write_action(status: i32) -> Raid1Write {
    match status {
        BIOC_SDONLINE | BIOC_SDSCRUB | BIOC_SDREBUILD => Raid1Write::Write,
        // BIOC_SDHOTSPARE should never happen
        BIOC_SDHOTSPARE | BIOC_SDOFFLINE => Raid1Write::Skip,
        _ => Raid1Write::Bad,
    }
}

/// `sr_raid1_rw`: a read becomes one ccb on the next readable chunk, a write one ccb on
/// every working chunk; the work unit is then scheduled.
pub fn sr_raid1_rw(wu: &'static SrWorkunit) -> Result<(), Errno> {
    let sd = wu.dis();
    let xs = wu.xs();

    // blkno and scsi error will be handled by sr_validate_io
    // (the work unit is unwound by sr_wu_put)
    let blkno = sr_validate_io(wu, "sr_raid1_rw")?;

    let no_chunk = sd.sd_meta().ssdi().ssd_chunk_no.get();
    let read = xs.flags.get() & SCSI_DATA_IN != 0;
    let ios = if read { 1 } else { no_chunk };

    for i in 0..ios {
        let chunk = if read {
            let counter = &sd.mds().mdd_raid1.sr1_counter;
            let Some(chunk) = raid1_read_chunk(counter, no_chunk, |chunk| {
                sd.sd_vol.sv_chunk(chunk).src_meta.scm_status.get() as i32
            }) else {
                // volume offline
                printf(format_args!(
                    "{}: is offline, cannot read\n",
                    DEVNAME(sd.sd_sc())
                ));
                return Err(Errno::EIO);
            };
            chunk
        } else {
            // writes go on all working disks
            let chunk = i as usize;
            let scp = sd.sd_vol.sv_chunk(chunk);
            match raid1_write_action(scp.src_meta.scm_status.get() as i32) {
                Raid1Write::Write => {}
                Raid1Write::Skip => continue,
                Raid1Write::Bad => return Err(Errno::EIO),
            }
            chunk
        };

        // SAFETY: `xs.data()` is valid for `datalen` bytes until the transfer completes
        // (`ScsiXfer::set_data`); every ccb of the work unit reads or writes the whole of it,
        // as the C does (a write only reads the buffer; a read has one ccb).
        let ccb = unsafe {
            sr_ccb_rw(
                sd,
                chunk,
                blkno,
                i64::from(xs.datalen()),
                xs.data(),
                xs.flags.get(),
                0,
            )
        };
        let Some(ccb) = ccb else {
            // should never happen but handle more gracefully
            printf(format_args!(
                "{}: {}: too many ccbs queued\n",
                DEVNAME(sd.sd_sc()),
                Name(sd.sd_meta().ssd_devname.get())
            ));
            return Err(Errno::EIO);
        };
        sr_wu_enqueue_ccb(wu, ccb);
    }

    sr_schedule_wu(wu);

    Ok(())
}

/// `sr_raid1_wu_done`: all of a work unit's I/Os are done: it is fine when at least one
/// succeeded; a read whose I/Os all failed is retried (on the next chunk) and a write is
/// given up on. Returns the work unit's new state.
pub fn sr_raid1_wu_done(wu: &'static SrWorkunit) -> i32 {
    let sd = wu.dis();
    let xs = wu.xs();

    // If at least one I/O succeeded, we are okay.
    if wu.swu_ios_succeeded.get() > 0 {
        xs.error.set(XS_NOERROR);
        return SR_WU_OK;
    }

    // If all I/O failed, retry reads and give up on writes.
    if xs.flags.get() & SCSI_DATA_IN != 0 {
        printf(format_args!(
            "{}: retrying read on block {}\n",
            Name(sd.sd_meta().ssd_devname.get()),
            wu.swu_blk_start.get()
        ));
        if wu.swu_cb_active.get() == 1 {
            panic(format_args!("{}: sr_raid1_intr_cb", DEVNAME(sd.sd_sc())));
        }
        sr_wu_release_ccbs(wu);
        wu.swu_state.set(SR_WU_RESTART);
        if sd.sd_scsi_rw(wu).is_ok() {
            return SR_WU_RESTART;
        }
    } else {
        printf(format_args!(
            "{}: permanently failing write on block {}\n",
            Name(sd.sd_meta().ssd_devname.get()),
            wu.swu_blk_start.get()
        ));
    }

    wu.swu_state.set(SR_WU_FAILED);
    xs.error.set(XS_DRIVER_STUFFUP);

    SR_WU_FAILED
}

#[cfg(test)]
mod tests;
