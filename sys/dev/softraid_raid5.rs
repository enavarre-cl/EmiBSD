/* $OpenBSD: softraid_raid5.c,v 1.33 2026/09/23 13:29:14 krw Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2014 Joel Sing <jsing@openbsd.org>
 * Copyright (c) 2009 Marco Peereboom <marco@peereboom.us>
 * Copyright (c) 2009 Jordan Hargrave <jordan@openbsd.org>
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

/* <CODE> */
//! `softraid_raid5.c`: the RAID 5 discipline of softraid(4). A volume of `n` chunks stores
//! its data in strips of `ssd_strip_size` bytes (`MAXPHYS`) dealt over `n - 1` chunks per
//! row, plus one parity strip per row (the XOR of the row's data strips) whose chunk rotates
//! left asymmetrically from the last chunk. A read of a strip on a missing chunk is rebuilt
//! from the other chunks of its row; a write reads what it needs to compute the new parity in
//! a second work unit that runs before it (the C's "collision").
//!
//! Upstream: sys/dev/softraid_raid5.c @ 3ce1f3f79392
//!
//! The arithmetic of a strip (which chunk holds it, which chunk holds the row's parity, the
//! block on the chunk) is [`Raid5Strip`]; `sr_raid5_regenerate` and `sr_raid5_write`, which
//! decide the ccbs of a strip, reach the chunks through the small [`Raid5Io`] trait, whose
//! kernel implementation is `sr_raid5_addio`'s ccbs and whose test implementation is a set of
//! in-memory chunks, so that the host tests run the same code on real bytes.
//!
//! ## Deviations
//! - Hooks return `Result<(), Errno>`; the C's `return (1)` and `sr_raid5_addio`'s `-1` are
//!   `Err(Errno::EIO)`; `sr_raid5_create` and `sr_raid5_init` keep their `EINVAL`.
//! - `sr_raid5_regenerate` and `sr_raid5_write` are generic over [`Raid5Io`] (the chunk
//!   status, `sr_raid5_addio`, `sr_block_get`/`put`, `memset`, `memcpy`) instead of taking a
//!   work unit's discipline; `sr_raid5_chunk_online`/`_rebuild` take the same `io`.
//!   `sr_raid5_write` uses its `xsflags` argument where the C reads `wu->swu_xs->flags` (the
//!   only caller passes `xs->flags`).
//! - Buffers are [`Raid5Buf`] (a pointer and its length, made by an `unsafe` constructor that
//!   states the C's validity contract) instead of `void *`.
//! - Leaks the C has on error paths are closed: `sr_raid5_write` puts its parity block back
//!   when a read for it cannot be queued, and `sr_raid5_rebuild` puts the strip's block back
//!   when `sr_raid5_regenerate` fails. The ccbs already queued on the work units then never
//!   run (the work units are put back unstarted), so nothing reads the block afterwards.
//! - `sr_raid5_rw` does not queue a write work unit on `sd_wu_defq` a second time when it is
//!   already there (`sr_raid_recreate_wu` re-running a deferred write): the C's
//!   `TAILQ_INSERT_TAIL` would corrupt the queue, which Rust's queue contract forbids; the
//!   unit stays where it was. A write that comes without a read work unit (no
//!   `SCSI_DATA_OUT`, where the C would dereference NULL) fails.
//! - `sr_raid5_xor` works on byte slices; like the C's 32-bit loop it XORs `len & !3` bytes.
//! - `sr_raid5_scrub` is `#if 0` in the C and is not ported.
//! - `DNPRINTF` calls are comments (`SR_DEBUG` is not configured).

use core::ptr::{self, NonNull};
use core::slice;

use crate::dev::biovar::{
    BIOC_SDHOTSPARE, BIOC_SDOFFLINE, BIOC_SDONLINE, BIOC_SDREBUILD, BIOC_SDSCRUB, BIOC_SVBUILDING,
    BIOC_SVDEGRADED, BIOC_SVOFFLINE, BIOC_SVONLINE, BIOC_SVREBUILD, BIOC_SVSCRUB, BiocCreateraid,
};
use crate::dev::softraid::{
    sr_block_get, sr_block_put, sr_ccb_done, sr_ccb_rw, sr_error, sr_meta_save, sr_rebuild_percent,
    sr_schedule_wu, sr_scsi_wu_get, sr_scsi_wu_put, sr_validate_io, sr_validate_stripsize,
    sr_wu_done, sr_wu_enqueue_ccb, sr_wu_release_ccbs,
};
use crate::dev::softraidvar::*;
use crate::kern::kern_synch::tsleep_nsec;
use crate::kern::kern_task::task_add;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::intr::{splbio, splx};
use crate::scsi::scsiconf::{
    SCSI_DATA_IN, SCSI_DATA_OUT, SCSI_NOSLEEP, XS_DRIVER_STUFFUP, XS_NOERROR,
};
use crate::sys::buf::Buf;
use crate::sys::errno::Errno;
use crate::sys::param::{DEV_BSHIFT, MAXPHYS, PRIBIO, PWAIT};
use crate::sys::systm::INFSLP;
use crate::sys::task::SYSTQ;
use crate::sys::time::msec_to_nsec;
use crate::sys::types::Daddr;

/// One strip of a RAID 5 transfer: where `sr_raid5_rw` finds it on the chunks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Raid5Strip {
    /// `strip_no`: the strip's number in the volume's data.
    pub(crate) strip_no: i64,
    /// `chunk`: the chunk that holds it.
    pub(crate) chunk: i64,
    /// `parity`: the chunk that holds its row's parity.
    pub(crate) parity: i64,
    /// `lba`: its block on both (relative to the data area).
    pub(crate) lba: Daddr,
    /// `length`: the bytes of the transfer in it.
    pub(crate) length: i64,
}

impl Raid5Strip {
    /// The strip at byte `lbaoffs` of the volume, `datalen` bytes of the transfer left;
    /// `no_chunk` is the number of data chunks of a row (`ssd_chunk_no - 1`).
    pub(crate) fn new(
        lbaoffs: i64,
        datalen: i64,
        strip_size: i64,
        strip_bits: i64,
        no_chunk: i64,
    ) -> Self {
        let strip_no = lbaoffs >> strip_bits;
        let strip_offs = lbaoffs & (strip_size - 1);
        let chunk_offs = (strip_no / no_chunk) << strip_bits;
        let offset = chunk_offs + strip_offs;

        // get size remaining in this stripe
        let length = (strip_size - strip_offs).min(datalen);

        // Map disk offset to data and parity chunks, using a left asymmetric algorithm for
        // the parity assignment.
        let mut chunk = strip_no % no_chunk;
        let parity = no_chunk - ((strip_no / no_chunk) % (no_chunk + 1));
        if chunk >= parity {
            chunk += 1;
        }

        Raid5Strip {
            strip_no,
            chunk,
            parity,
            lba: offset >> DEV_BSHIFT,
            length,
        }
    }
}

/// A buffer of a strip's I/O (the C's `void *`): the transfer's data at the strip, or an
/// `sr_block_get` block.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Raid5Buf {
    /// The first byte.
    ptr: NonNull<u8>,
    /// The bytes it may be used for.
    len: usize,
}

impl Raid5Buf {
    /// The buffer of `len` bytes at `ptr`; `None` for NULL or a negative length.
    ///
    /// # Safety
    ///
    /// `ptr` is valid for reads and writes of `len` bytes, which nothing but this discipline's
    /// I/O touches, until the I/O that uses the buffer has completed and (for a block) the
    /// block has been put back.
    unsafe fn new(ptr: *mut u8, len: i64) -> Option<Self> {
        Some(Raid5Buf {
            ptr: NonNull::new(ptr)?,
            len: usize::try_from(len).ok()?,
        })
    }

    /// The buffer's address.
    fn as_ptr(self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// `len` as a byte count of this buffer; panics past its end.
    fn span(self, len: i64) -> usize {
        match usize::try_from(len) {
            Ok(n) if n <= self.len => n,
            _ => panic(format_args!(
                "sr_raid5: {} bytes past a buffer of {}",
                len, self.len
            )),
        }
    }
}

/// The kernel's [`Raid5Io`]: ccbs on the chunks of a discipline.
struct Raid5Ccbs {
    /// The discipline.
    sd: &'static SrDiscipline,
}

/// The chunk I/O that `sr_raid5_regenerate` and `sr_raid5_write` build a strip's ccbs from.
pub(crate) trait Raid5Io {
    /// A work unit (`struct sr_workunit *`).
    type Wu: Copy;
    /// A buffer (`void *`).
    type Buf: Copy;

    /// `sd->sd_meta->ssdi.ssd_chunk_no`.
    fn chunk_no(&self) -> i64;
    /// `sd->sd_vol.sv_chunks[chunk]->src_meta.scm_status`.
    fn chunk_status(&self, chunk: i64) -> i32;
    /// `sr_raid5_addio`: queues on `wu` a read (`SCSI_DATA_IN`) or write of `len` bytes at
    /// `blkno` of `chunk` into or from `data` (`None`: a block of its own, freed when the
    /// ccb completes), whose data is XORed into `xorbuf` when it completes.
    #[allow(clippy::too_many_arguments)] // the C's signature
    fn addio(
        &mut self,
        wu: Self::Wu,
        chunk: i64,
        blkno: Daddr,
        len: i64,
        data: Option<Self::Buf>,
        xsflags: i32,
        ccbflags: i32,
        xorbuf: Option<Self::Buf>,
    ) -> Result<(), Errno>;
    /// `sr_block_get`.
    fn block_get(&mut self, len: i64) -> Option<Self::Buf>;
    /// `sr_block_put`.
    fn block_put(&mut self, buf: Self::Buf, len: i64);
    /// `memset(buf, 0, len)`.
    fn zero(&mut self, buf: Self::Buf, len: i64);
    /// `memcpy(dst, src, len)`.
    fn copy(&mut self, dst: Self::Buf, src: Self::Buf, len: i64);
}

/// `sr_raid5_discipline_init`: discipline initialisation.
pub fn sr_raid5_discipline_init(sd: &'static SrDiscipline) {
    // Fill out discipline members.
    sd.sd_type.set(SR_MD_RAID5);
    sd.sd_name.set(*b"RAID 5\0\0\0\0");
    sd.sd_capabilities
        .set(SR_CAP_SYSTEM_DISK | SR_CAP_AUTO_ASSEMBLE | SR_CAP_REBUILD | SR_CAP_REDUNDANT);
    sd.sd_max_wu.set(SR_RAID5_NOWU + 2); // Two for scrub/rebuild.

    // Setup discipline specific function pointers.
    sd.sd_assemble.set(Some(sr_raid5_assemble));
    sd.sd_create.set(Some(sr_raid5_create));
    sd.sd_openings.set(Some(sr_raid5_openings));
    sd.sd_rebuild.set(Some(sr_raid5_rebuild));
    sd.sd_scsi_rw.set(Some(sr_raid5_rw));
    sd.sd_scsi_intr.set(Some(sr_raid5_intr));
    sd.sd_scsi_wu_done.set(Some(sr_raid5_wu_done));
    sd.sd_set_chunk_state.set(Some(sr_raid5_set_chunk_state));
    sd.sd_set_vol_state.set(Some(sr_raid5_set_vol_state));
}

/// `sr_raid5_create`: sets up the metadata of a new RAID 5 volume of `no_chunk` chunks of
/// `coerced_size` blocks.
pub fn sr_raid5_create(
    sd: &'static SrDiscipline,
    _bc: &mut BiocCreateraid,
    no_chunk: i32,
    coerced_size: i64,
) -> Result<(), Errno> {
    if no_chunk < 3 {
        sr_error(
            sd.sd_sc(),
            format_args!("{} requires three or more chunks", sd.name()),
        );
        return Err(Errno::EINVAL);
    }

    // XXX add variable strip size later even though MAXPHYS is really the clever value,
    // users like to tinker with that type of stuff.
    let ssdi = sd.sd_meta().ssdi();
    ssdi.ssd_strip_size.set(MAXPHYS as u32);
    ssdi.ssd_size.set(raid5_volume_size(
        coerced_size,
        ssdi.ssd_strip_size.get(),
        no_chunk,
    ));

    sr_raid5_init(sd)
}

/// The size in blocks of a RAID 5 volume of `no_chunk` chunks of `coerced_size` blocks: the
/// chunk size truncated to whole strips, times the `no_chunk - 1` data chunks of a row.
pub(crate) fn raid5_volume_size(coerced_size: i64, strip_size: u32, no_chunk: i32) -> i64 {
    let strip_blocks = u64::from(strip_size) >> DEV_BSHIFT;
    ((coerced_size as u64 & !(strip_blocks.wrapping_sub(1)))
        .wrapping_mul(i64::from(no_chunk - 1) as u64)) as i64
}

/// `sr_raid5_assemble`: brings up an existing RAID 5 volume.
pub fn sr_raid5_assemble(
    sd: &'static SrDiscipline,
    _bc: &mut BiocCreateraid,
    _no_chunk: i32,
    _data: Option<&[u8]>,
) -> Result<(), Errno> {
    sr_raid5_init(sd)
}

/// `sr_raid5_init`: initialises the runtime values (the strip shift and the ccb budget).
pub fn sr_raid5_init(sd: &'static SrDiscipline) -> Result<(), Errno> {
    let ssdi = sd.sd_meta().ssdi();

    // Initialise runtime values.
    let strip_bits = sr_validate_stripsize(ssdi.ssd_strip_size.get()).unwrap_or(-1);
    sd.mds().mdd_raid5.sr5_strip_bits.set(strip_bits);
    if strip_bits == -1 {
        sr_error(sd.sd_sc(), format_args!("invalid strip size"));
        return Err(Errno::EINVAL);
    }

    sd.sd_max_ccb_per_wu.set(ssdi.ssd_chunk_no.get());

    Ok(())
}

/// `sr_raid5_openings`: two work units per I/O, two for rebuild/scrub.
pub fn sr_raid5_openings(sd: &'static SrDiscipline) -> i32 {
    (sd.sd_max_wu.get().saturating_sub(2) >> 1) as i32
}

/// Whether a RAID 5 chunk may go from `old_state` to `new_state` (`BIOC_SD*`, different).
pub(crate) fn raid5_chunk_state_ok(old_state: i32, new_state: i32) -> bool {
    match old_state {
        BIOC_SDONLINE => matches!(new_state, BIOC_SDOFFLINE | BIOC_SDSCRUB),
        BIOC_SDOFFLINE => new_state == BIOC_SDREBUILD,
        BIOC_SDSCRUB | BIOC_SDREBUILD => matches!(new_state, BIOC_SDONLINE | BIOC_SDOFFLINE),
        _ => false,
    }
}

/// `sr_raid5_set_chunk_state`: chunk `c` goes to `new_state`; panics on a transition RAID 5
/// does not know.
pub fn sr_raid5_set_chunk_state(sd: &'static SrDiscipline, c: usize, new_state: i32) {
    let chunk = sd.sd_vol.sv_chunk(c);

    // DNPRINTF(SR_D_STATE, "%s: %s: %s: sr_raid_set_chunk_state %d -> %d")

    // ok to go to splbio since this only happens in error path
    let s = splbio();
    let old_state = chunk.src_meta.scm_status.get() as i32;

    // multiple IOs to the same chunk that fail will come through here
    if old_state != new_state {
        if !raid5_chunk_state_ok(old_state, new_state) {
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

        chunk.src_meta.scm_status.set(new_state as u32);
        sd.sd_set_vol_state();

        sd.sd_must_flush.set(1);
        let _ = task_add(SYSTQ, &sd.sd_meta_save_task);
    }

    // done:
    splx(s);
}

/// The state of a RAID 5 volume of `nd` chunks that count `states[s]` chunks in each state
/// `s`; `None` when no state fits (the C panics).
pub(crate) fn raid5_vol_state(states: &[i64; SR_MAX_STATES], nd: i64) -> Option<i32> {
    let online = states[BIOC_SDONLINE as usize];
    if online == nd {
        Some(BIOC_SVONLINE)
    } else if online < nd - 1 {
        Some(BIOC_SVOFFLINE)
    } else if states[BIOC_SDSCRUB as usize] != 0 {
        Some(BIOC_SVSCRUB)
    } else if states[BIOC_SDREBUILD as usize] != 0 {
        Some(BIOC_SVREBUILD)
    } else if online == nd - 1 {
        Some(BIOC_SVDEGRADED)
    } else {
        None
    }
}

/// Whether a RAID 5 volume may go from `old_state` to `new_state` (`BIOC_SV*`).
pub(crate) fn raid5_vol_state_ok(old_state: i32, new_state: i32) -> bool {
    match old_state {
        // can go to same state; BIOC_SVREBUILD happens on boot
        BIOC_SVONLINE => matches!(
            new_state,
            BIOC_SVONLINE | BIOC_SVOFFLINE | BIOC_SVDEGRADED | BIOC_SVREBUILD
        ),
        // XXX this might be a little too much
        BIOC_SVOFFLINE => false,
        // can go to the same state
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

/// `sr_raid5_set_vol_state`: the volume's state from its chunks'; panics on a transition
/// RAID 5 does not know.
pub fn sr_raid5_set_vol_state(sd: &'static SrDiscipline) {
    let mut states = [0i64; SR_MAX_STATES];
    let old_state = sd.sd_vol_status.get();

    // DNPRINTF(SR_D_STATE, "%s: %s: sr_raid_set_vol_state")

    let nd = sd.sd_meta().ssdi().ssd_chunk_no.get() as usize;

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

    let Some(new_state) = raid5_vol_state(&states, nd as i64) else {
        // SR_DEBUG: DNPRINTF(SR_D_STATE, "%s: invalid volume state, old state was %d") and
        // each chunk's status.
        panic(format_args!("invalid volume state"));
    };

    // DNPRINTF(SR_D_STATE, "%s: %s: sr_raid5_set_vol_state %d -> %d")

    if !raid5_vol_state_ok(old_state, new_state) {
        panic(format_args!(
            "{}: {}: invalid volume state transition {} -> {}",
            DEVNAME(sd.sd_sc()),
            Name(sd.sd_meta().ssd_devname.get()),
            old_state,
            new_state
        ));
    }

    sd.sd_vol_status.set(new_state);
}

/// `sr_raid5_chunk_online`: the chunk takes I/O (online or being scrubbed).
fn sr_raid5_chunk_online<I: Raid5Io>(io: &I, chunk: i64) -> bool {
    matches!(io.chunk_status(chunk), BIOC_SDONLINE | BIOC_SDSCRUB)
}

/// `sr_raid5_chunk_rebuild`: the chunk is being rebuilt.
fn sr_raid5_chunk_rebuild<I: Raid5Io>(io: &I, chunk: i64) -> bool {
    io.chunk_status(chunk) == BIOC_SDREBUILD
}

/// `sr_raid5_rw`: the `sd_scsi_rw` hook: splits a read or write into strips and queues the
/// ccbs of each; a write gets a second work unit for the reads its parity needs, which runs
/// first.
pub fn sr_raid5_rw(wu: &'static SrWorkunit) -> Result<(), Errno> {
    let sd = wu.dis();
    let xs = wu.xs();

    // blkno and scsi error will be handled by sr_validate_io
    let blkno = sr_validate_io(wu, "sr_raid5_rw")?;

    // DNPRINTF(SR_D_DIS, "%s: %s sr_raid5_rw %s: blkno %lld size %d")

    let mut wu_r: Option<&'static SrWorkunit> = None;
    if xs.flags.get() & SCSI_DATA_OUT != 0 {
        let Some(r) = sr_scsi_wu_get(sd, SCSI_NOSLEEP) else {
            printf(format_args!(
                "{}: {} failed to get read work unit",
                DEVNAME(sd.sd_sc()),
                Name(sd.sd_meta().ssd_devname.get())
            ));
            return Err(Errno::EIO);
        };
        r.swu_state.set(SR_WU_INPROGRESS);
        r.swu_flags.set(r.swu_flags.get() | SR_WUF_DISCIPLINE);
        wu_r = Some(r);
    }

    if let Err(e) = sr_raid5_rw_strips(sd, wu, wu_r, blkno) {
        // wu is unwound by sr_wu_put
        if let Some(r) = wu_r {
            sr_scsi_wu_put(sd, r);
        }
        return Err(e);
    }

    let mut sched = wu;
    let s = splbio();
    if let Some(r) = wu_r {
        if r.swu_io_count.get() > 0 {
            // collide write request with reads
            r.swu_blk_start.set(wu.swu_blk_start.get());
            r.swu_blk_end.set(wu.swu_blk_end.get());

            wu.swu_state.set(SR_WU_DEFERRED);
            r.swu_collider.set(Some(wu));
            if !sd.sd_wu_defq.iter().any(|w| ptr::eq(w, wu)) {
                // SAFETY: the work unit is on no other processing queue (being built, or
                // restarted off the pending queue), and not on this one (checked); work
                // units live until `sr_wu_free`; at `splbio`.
                unsafe { sd.sd_wu_defq.insert_tail(wu) };
            }

            sched = r;
        } else {
            sr_scsi_wu_put(sd, r);
        }
    }
    splx(s);

    sr_schedule_wu(sched);

    Ok(())
}

/// The strip loop of `sr_raid5_rw`: queues the ccbs of every strip of the transfer of `wu`
/// at volume block `blkno`.
fn sr_raid5_rw_strips(
    sd: &'static SrDiscipline,
    wu: &'static SrWorkunit,
    wu_r: Option<&'static SrWorkunit>,
    blkno: Daddr,
) -> Result<(), Errno> {
    let xs = wu.xs();
    let ssdi = sd.sd_meta().ssdi();
    let mut io = Raid5Ccbs { sd };

    let strip_size = i64::from(ssdi.ssd_strip_size.get());
    let strip_bits = i64::from(sd.mds().mdd_raid5.sr5_strip_bits.get());
    let no_chunk = i64::from(ssdi.ssd_chunk_no.get()) - 1;
    let row_size = (no_chunk << strip_bits) >> DEV_BSHIFT;

    let data = xs.data();
    let mut datalen = i64::from(xs.datalen());
    let mut lbaoffs = blkno << DEV_BSHIFT;
    let mut done: usize = 0;

    wu.swu_blk_start.set(0);
    while datalen != 0 {
        let st = Raid5Strip::new(lbaoffs, datalen, strip_size, strip_bits, no_chunk);
        let row = st.strip_no / no_chunk;

        // XXX big hammer.. exclude I/O from entire stripe
        if wu.swu_blk_start.get() == 0 {
            wu.swu_blk_start.set(row * row_size);
        }
        wu.swu_blk_end.set(row * row_size + (row_size - 1));

        // SAFETY: `done + st.length <= xs->datalen`, and the transfer's data is valid for
        // `datalen` bytes until it completes (`ScsiXfer::set_data`), which is after every ccb
        // of this work unit has; the discipline is the transfer's adapter meanwhile.
        let Some(buf) = (unsafe { Raid5Buf::new(data.wrapping_add(done), st.length) }) else {
            return Err(Errno::EIO);
        };

        if xs.flags.get() & SCSI_DATA_IN != 0 {
            match io.chunk_status(st.chunk) {
                BIOC_SDONLINE | BIOC_SDSCRUB => {
                    // Chunk is online, issue a single read request.
                    io.addio(
                        wu,
                        st.chunk,
                        st.lba,
                        st.length,
                        Some(buf),
                        xs.flags.get(),
                        0,
                        None,
                    )?;
                }
                BIOC_SDOFFLINE | BIOC_SDREBUILD | BIOC_SDHOTSPARE => {
                    sr_raid5_regenerate(&mut io, wu, st.chunk, st.lba, st.length, buf)?;
                }
                _ => {
                    printf(format_args!(
                        "{}: is offline, can't read\n",
                        DEVNAME(sd.sd_sc())
                    ));
                    return Err(Errno::EIO);
                }
            }
        } else {
            let Some(r) = wu_r else {
                return Err(Errno::EIO);
            };
            sr_raid5_write(
                &mut io,
                wu,
                r,
                st.chunk,
                st.parity,
                st.lba,
                st.length,
                buf,
                xs.flags.get(),
                0,
            )?;
        }

        // advance to next block
        lbaoffs += st.length;
        datalen -= st.length;
        done += st.length as usize;
    }

    Ok(())
}

/// `sr_raid5_regenerate`: regenerates a block on a RAID 5 volume by xoring the data and
/// parity from all of the remaining online chunks into `data`. This requires the parity to
/// already be correct.
pub(crate) fn sr_raid5_regenerate<I: Raid5Io>(
    io: &mut I,
    wu: I::Wu,
    chunk: i64,
    blkno: Daddr,
    len: i64,
    data: I::Buf,
) -> Result<(), Errno> {
    // DNPRINTF(SR_D_DIS, "%s: %s sr_raid5_regenerate chunk %d offline, regenerating block %llu")

    io.zero(data, len);
    for i in 0..io.chunk_no() {
        if i == chunk {
            continue;
        }
        if !sr_raid5_chunk_online(io, i) {
            return Err(Errno::EIO);
        }
        io.addio(wu, i, blkno, len, None, SCSI_DATA_IN, 0, Some(data))?;
    }

    Ok(())
}

/// `sr_raid5_write`: performs a write of strip data to `chunk` of a RAID 5 volume and the
/// update of the row's `parity` chunk. This write routine does not require the parity to
/// already be correct and will operate on an uninitialised volume.
///
/// There are four possible cases:
///
/// 1) All data chunks and parity are online. In this case we read the data from all data
///    chunks, except the one we are writing to, in order to calculate and write the new
///    parity.
///
/// 2) The parity chunk is offline. In this case we only need to write to the data chunk. No
///    parity calculation is required.
///
/// 3) The data chunk is offline. In this case we read the data from all online chunks in
///    order to calculate and write the new parity. This is the same as (1) except we do not
///    write the data chunk.
///
/// 4) A different data chunk is offline. The new parity is calculated by taking the existing
///    parity, xoring the original data and xoring in the new data. This requires that the
///    parity already be correct, which it will be if any of the data chunks has previously
///    been written.
///
/// There is an additional complication introduced by a chunk that is being rebuilt. If this
/// is the data or parity chunk, then we want to write to it as per normal. If it is another
/// data chunk then we need to presume that it has not yet been regenerated and use the same
/// method as detailed in (4) above.
#[allow(clippy::too_many_arguments)] // the C's signature
pub(crate) fn sr_raid5_write<I: Raid5Io>(
    io: &mut I,
    wu: I::Wu,
    wu_r: I::Wu,
    chunk: i64,
    parity: i64,
    blkno: Daddr,
    len: i64,
    data: I::Buf,
    xsflags: i32,
    _ccbflags: i32,
) -> Result<(), Errno> {
    // DNPRINTF(SR_D_DIS, "%s: %s sr_raid5_write chunk %i parity %i blkno %llu")

    let chunk_online = sr_raid5_chunk_online(io, chunk);
    let chunk_rebuild = sr_raid5_chunk_rebuild(io, chunk);
    let parity_online = sr_raid5_chunk_online(io, parity);
    let parity_rebuild = sr_raid5_chunk_rebuild(io, parity);
    let mut other_offline = false;
    let mut other_rebuild = false;

    for i in 0..io.chunk_no() {
        if i == chunk || i == parity {
            continue;
        }
        if sr_raid5_chunk_rebuild(io, i) {
            other_rebuild = true;
        } else if !sr_raid5_chunk_online(io, i) {
            other_offline = true;
        }
    }

    // DNPRINTF(SR_D_DIS, "%s: %s chunk online %d, parity online %d, other offline %d")

    if parity_online || parity_rebuild {
        let Some(xorbuf) = io.block_get(len) else {
            return Err(Errno::EIO);
        };
        io.copy(xorbuf, data, len);

        let reads: Result<(), Errno> = 'reads: {
            if other_offline || other_rebuild {
                // XXX - If we can guarantee that this LBA has been scrubbed then we can also
                // take this faster path.

                // Read in existing data and existing parity.
                for c in [chunk, parity] {
                    if let Err(e) =
                        io.addio(wu_r, c, blkno, len, None, SCSI_DATA_IN, 0, Some(xorbuf))
                    {
                        break 'reads Err(e);
                    }
                }
            } else {
                // Read in existing data from all other chunks.
                for i in 0..io.chunk_no() {
                    if i == chunk || i == parity {
                        continue;
                    }
                    if let Err(e) =
                        io.addio(wu_r, i, blkno, len, None, SCSI_DATA_IN, 0, Some(xorbuf))
                    {
                        break 'reads Err(e);
                    }
                }
            }
            Ok(())
        };
        if let Err(e) = reads {
            // The reads queued so far never run: their work unit is put back unstarted.
            io.block_put(xorbuf, len);
            return Err(e);
        }

        // Write new parity.
        io.addio(
            wu,
            parity,
            blkno,
            len,
            Some(xorbuf),
            xsflags,
            SR_CCBF_FREEBUF,
            None,
        )?;
    }

    // data_write:
    // Write new data.
    if chunk_online || chunk_rebuild {
        io.addio(wu, chunk, blkno, len, Some(data), xsflags, 0, None)?;
    }

    Ok(())
}

/// `sr_raid5_intr`: the `sd_scsi_intr` hook: a ccb is done; XORs a read into its
/// `ccb_opaque` buffer and frees its own block.
pub fn sr_raid5_intr(bp: &'static Buf) {
    // SAFETY: `sr_raid5_intr` is only installed as `sd_scsi_intr`, which `sr_ccb_rw` makes
    // the `b_iodone` of a ccb's own buffer.
    let ccb = unsafe { sr_ccb_from_buf(bp) };
    let wu = ccb.wu();
    let sd = wu.dis();

    // DNPRINTF(SR_D_INTR, "%s: sr_raid5_intr bp %p xs %p")

    let s = splbio();
    sr_ccb_done(ccb);

    // XXX - Should this be done via the taskq?

    // XOR data to result.
    let xorbuf = ccb.ccb_opaque.get().cast::<u8>();
    let len = usize::try_from(ccb.ccb_buf.b_bcount.get()).unwrap_or(0);
    let b_data = ccb.ccb_buf.b_data.get();
    if ccb.ccb_state.get() == SR_CCB_OK && !xorbuf.is_null() && !b_data.is_null() {
        // SAFETY: `sr_raid5_addio` sets `ccb_opaque` only for a read into a block of its own
        // (`data` NULL), so `b_data` is that block of `b_bcount` bytes, and `ccb_opaque` is a
        // different buffer (the transfer's data or a parity block) of the same length, both
        // valid until the work unit completes, which waits for this ccb; ccbs complete one
        // at a time at `splbio`.
        let (a, b) = unsafe {
            (
                slice::from_raw_parts_mut(xorbuf, len),
                slice::from_raw_parts(b_data.cast_const(), len),
            )
        };
        sr_raid5_xor(a, b, len);
    }

    // Free allocated data buffer.
    if ccb.ccb_flags.get() & SR_CCBF_FREEBUF != 0 {
        if let Some(p) = NonNull::new(b_data) {
            sr_block_put(sd, p, ccb.ccb_buf.b_bcount.get());
        }
        ccb.ccb_buf.b_data.set(ptr::null_mut());
    }

    sr_wu_done(wu);
    splx(s);
}

/// `sr_raid5_wu_done`: the `sd_scsi_wu_done` hook: a transfer succeeds when any of its ccbs
/// did; a failed read is restarted once more.
pub fn sr_raid5_wu_done(wu: &'static SrWorkunit) -> i32 {
    let sd = wu.dis();

    // XXX - we have no way of propagating errors...
    if wu.swu_flags.get() & (SR_WUF_DISCIPLINE | SR_WUF_REBUILD) != 0 {
        return SR_WU_OK;
    }

    let xs = wu.xs();

    // XXX - This is insufficient for RAID 5.
    if wu.swu_ios_succeeded.get() > 0 {
        xs.error.set(XS_NOERROR);
        return SR_WU_OK;
    }

    if xs.flags.get() & SCSI_DATA_IN != 0 {
        printf(format_args!(
            "{}: retrying read on block {}\n",
            Name(sd.sd_meta().ssd_devname.get()),
            wu.swu_blk_start.get()
        ));
        sr_wu_release_ccbs(wu);
        wu.swu_state.set(SR_WU_RESTART);
        if sd.sd_scsi_rw(wu).is_ok() {
            return SR_WU_RESTART;
        }
    } else {
        // XXX - retry write if we just went from online to degraded.
        printf(format_args!(
            "{}: permanently fail write on block {}\n",
            Name(sd.sd_meta().ssd_devname.get()),
            wu.swu_blk_start.get()
        ));
    }

    wu.swu_state.set(SR_WU_FAILED);
    xs.error.set(XS_DRIVER_STUFFUP);

    SR_WU_FAILED
}

/// `sr_raid5_addio`: queues on `wu` a ccb for `len` bytes at `blkno` of `chunk` (see
/// [`Raid5Io::addio`]).
#[allow(clippy::too_many_arguments)] // the C's signature
fn sr_raid5_addio(
    wu: &'static SrWorkunit,
    chunk: i64,
    blkno: Daddr,
    len: i64,
    data: Option<Raid5Buf>,
    xsflags: i32,
    mut ccbflags: i32,
    xorbuf: Option<Raid5Buf>,
) -> Result<(), Errno> {
    let sd = wu.dis();

    // DNPRINTF(SR_D_DIS, "sr_raid5_addio: %s chunk %d block %lld length %ld %s")

    // Allocate temporary buffer.
    let data = match data {
        Some(d) => d,
        None => {
            let Some(block) = sr_block_get(sd, len) else {
                return Err(Errno::EIO);
            };
            ccbflags |= SR_CCBF_FREEBUF;
            // SAFETY: a fresh block of `len` bytes, owned by the ccb until its interrupt
            // puts it back.
            match unsafe { Raid5Buf::new(block.as_ptr(), len) } {
                Some(b) => b,
                None => {
                    sr_block_put(sd, block, len);
                    return Err(Errno::EIO);
                }
            }
        }
    };
    let n = data.span(len);

    let Ok(chunk) = usize::try_from(chunk) else {
        return Err(Errno::EIO);
    };
    // SAFETY: `data` is valid for `n == len` bytes until the ccb completes (`Raid5Buf`'s
    // contract: the transfer's data, or a block the ccb frees when done).
    let ccb = unsafe { sr_ccb_rw(sd, chunk, blkno, n as i64, data.as_ptr(), xsflags, ccbflags) };
    let Some(ccb) = ccb else {
        if ccbflags & SR_CCBF_FREEBUF != 0 {
            sr_block_put(sd, data.ptr, len);
        }
        return Err(Errno::EIO);
    };
    ccb.ccb_opaque
        .set(xorbuf.map_or(ptr::null_mut(), |x| x.as_ptr().cast()));
    sr_wu_enqueue_ccb(wu, ccb);

    Ok(())
}

impl Raid5Io for Raid5Ccbs {
    type Wu = &'static SrWorkunit;
    type Buf = Raid5Buf;

    fn chunk_no(&self) -> i64 {
        i64::from(self.sd.sd_meta().ssdi().ssd_chunk_no.get())
    }

    fn chunk_status(&self, chunk: i64) -> i32 {
        self.sd
            .sd_vol
            .sv_chunk(chunk as usize)
            .src_meta
            .scm_status
            .get() as i32
    }

    fn addio(
        &mut self,
        wu: Self::Wu,
        chunk: i64,
        blkno: Daddr,
        len: i64,
        data: Option<Raid5Buf>,
        xsflags: i32,
        ccbflags: i32,
        xorbuf: Option<Raid5Buf>,
    ) -> Result<(), Errno> {
        sr_raid5_addio(wu, chunk, blkno, len, data, xsflags, ccbflags, xorbuf)
    }

    fn block_get(&mut self, len: i64) -> Option<Raid5Buf> {
        let block = sr_block_get(self.sd, len)?;
        // SAFETY: a fresh block of `len` bytes; whoever takes it puts it back.
        let buf = unsafe { Raid5Buf::new(block.as_ptr(), len) };
        if buf.is_none() {
            sr_block_put(self.sd, block, len);
        }
        buf
    }

    fn block_put(&mut self, buf: Raid5Buf, len: i64) {
        sr_block_put(self.sd, buf.ptr, len);
    }

    fn zero(&mut self, buf: Raid5Buf, len: i64) {
        let n = buf.span(len);
        // SAFETY: `buf` is valid for writes of its `len >= n` bytes (`Raid5Buf`).
        unsafe { ptr::write_bytes(buf.as_ptr(), 0, n) };
    }

    fn copy(&mut self, dst: Raid5Buf, src: Raid5Buf, len: i64) {
        let n = dst.span(len).min(src.span(len));
        // SAFETY: both are valid for `n` bytes (`Raid5Buf`); `ptr::copy` allows overlap.
        unsafe { ptr::copy(src.as_ptr(), dst.as_ptr(), n) };
    }
}

/// `sr_raid5_xor`: `a ^= b` over the first `len` bytes, in the C's 32-bit words (`len & !3`
/// bytes).
pub(crate) fn sr_raid5_xor(a: &mut [u8], b: &[u8], len: usize) {
    let n = len & !3;
    for (x, y) in a[..n].iter_mut().zip(&b[..n]) {
        *x ^= *y;
    }
}

/// `sr_raid5_rebuild`: the `sd_rebuild` hook: regenerates the chunk being rebuilt strip by
/// strip from the others, saving the progress in the metadata.
pub fn sr_raid5_rebuild(sd: &'static SrDiscipline) {
    let sc = sd.sd_sc();
    let meta = sd.sd_meta();
    let ssdi = meta.ssdi();
    let mut io = Raid5Ccbs { sd };
    let mut wu_r: Option<&'static SrWorkunit> = None;
    let mut wu_w: Option<&'static SrWorkunit> = None;
    let mut percent;
    let mut old_percent = -1;

    'bad: {
        // Find the rebuild chunk.
        let Some(rebuild_chunk) = (0..io.chunk_no()).find(|&i| sr_raid5_chunk_rebuild(&io, i))
        else {
            break 'bad;
        };

        let strip_size = i64::from(ssdi.ssd_strip_size.get());
        let strip_bits = i64::from(sd.mds().mdd_raid5.sr5_strip_bits.get());
        let chunk_count = io.chunk_no() - 1;
        let chunk_size = ssdi.ssd_size.get() / chunk_count;
        let chunk_strips = (chunk_size << DEV_BSHIFT) >> strip_bits;
        let row_size = (chunk_count << strip_bits) >> DEV_BSHIFT;

        // DNPRINTF(SR_D_REBUILD, "%s: %s sr_raid5_rebuild volume size = %lld, ...")

        let mut restart = meta.ssd_rebuild.get() / row_size;
        if restart > chunk_strips {
            printf(format_args!(
                "{}: bogus rebuild restart offset, starting from 0\n",
                DEVNAME(sc)
            ));
            restart = 0;
        }
        if restart != 0 {
            percent = sr_rebuild_percent(sd);
            printf(format_args!(
                "{}: resuming rebuild on {} at {}%\n",
                DEVNAME(sc),
                Name(meta.ssd_devname.get()),
                percent
            ));
        }

        'abort: {
            for strip_no in restart..chunk_strips {
                let chunk_lba = (strip_size >> DEV_BSHIFT) * strip_no;

                // DNPRINTF(SR_D_REBUILD, "%s: %s rebuild strip %lld, chunk lba = %lld")

                wu_w = sr_scsi_wu_get(sd, 0);
                wu_r = sr_scsi_wu_get(sd, 0);
                let (Some(w), Some(r)) = (wu_w, wu_r) else {
                    break 'bad;
                };

                let Some(xorbuf) = io.block_get(strip_size) else {
                    break 'bad;
                };
                if sr_raid5_regenerate(&mut io, r, rebuild_chunk, chunk_lba, strip_size, xorbuf)
                    .is_err()
                {
                    io.block_put(xorbuf, strip_size);
                    break 'bad;
                }
                if sr_raid5_addio(
                    w,
                    rebuild_chunk,
                    chunk_lba,
                    strip_size,
                    Some(xorbuf),
                    SCSI_DATA_OUT,
                    SR_CCBF_FREEBUF,
                    None,
                )
                .is_err()
                {
                    break 'bad;
                }

                // Collide write work unit with read work unit.
                r.swu_state.set(SR_WU_INPROGRESS);
                r.swu_flags.set(r.swu_flags.get() | SR_WUF_REBUILD);
                w.swu_state.set(SR_WU_DEFERRED);
                w.swu_flags
                    .set(w.swu_flags.get() | SR_WUF_REBUILD | SR_WUF_WAKEUP);
                r.swu_collider.set(Some(w));

                // Block I/O to this strip while we rebuild it.
                r.swu_blk_start.set((strip_no / chunk_count) * row_size);
                r.swu_blk_end.set(r.swu_blk_start.get() + row_size - 1);
                w.swu_blk_start.set(r.swu_blk_start.get());
                w.swu_blk_end.set(r.swu_blk_end.get());

                // DNPRINTF(SR_D_REBUILD, "%s: %s rebuild swu_blk_start = %lld, swu_blk_end = %lld")

                let s = splbio();
                // SAFETY: a work unit just taken from the pool is on no processing queue;
                // work units live until `sr_wu_free`; at `splbio`.
                unsafe { sd.sd_wu_defq.insert_tail(w) };
                splx(s);

                sr_schedule_wu(r);

                let mut slept = false;
                while w.swu_flags.get() & SR_WUF_REBUILDIOCOMP == 0 {
                    let _ = tsleep_nsec(ptr::from_ref(w), PRIBIO, "sr_rebuild", INFSLP);
                    slept = true;
                }
                if !slept {
                    let _ = tsleep_nsec(ptr::from_ref(sc), PWAIT, "sr_yield", msec_to_nsec(1));
                }

                sr_scsi_wu_put(sd, r);
                sr_scsi_wu_put(sd, w);
                wu_r = None;
                wu_w = None;

                meta.ssd_rebuild.set(chunk_lba * chunk_count);

                percent = sr_rebuild_percent(sd);
                if percent != old_percent && strip_no != chunk_strips - 1 {
                    if sr_meta_save(sd, SR_META_DIRTY).is_err() {
                        printf(format_args!(
                            "{}: could not save metadata to {}\n",
                            DEVNAME(sc),
                            Name(meta.ssd_devname.get())
                        ));
                    }
                    old_percent = percent;
                }

                if sd.sd_reb_abort.get() != 0 {
                    break 'abort;
                }
            }

            // DNPRINTF(SR_D_REBUILD, "%s: %s rebuild complete")

            // all done
            meta.ssd_rebuild.set(0);
            for i in 0..io.chunk_no() {
                if sr_raid5_chunk_rebuild(&io, i) {
                    sd.sd_set_chunk_state(i as usize, BIOC_SDONLINE);
                    break;
                }
            }

            return;
        }

        // abort:
        if sr_meta_save(sd, SR_META_DIRTY).is_err() {
            printf(format_args!(
                "{}: could not save metadata to {}\n",
                DEVNAME(sc),
                Name(meta.ssd_devname.get())
            ));
        }
    }

    // bad:
    if let Some(r) = wu_r {
        sr_scsi_wu_put(sd, r);
    }
    if let Some(w) = wu_w {
        sr_scsi_wu_put(sd, w);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    extern crate std;
    use std::vec;
    use std::vec::Vec;

    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::sys::malloc::M_WAITOK;
    use crate::sys::param::DEV_BSIZE;

    /// The strip size of the in-memory volumes (small, to keep them small).
    const STRIP: i64 = 4096;
    /// Its shift.
    const BITS: i64 = 12;

    /// A deterministic byte stream (an LCG), for test data.
    fn bytes(seed: u32, n: usize) -> Vec<u8> {
        let mut x = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
        (0..n)
            .map(|_| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (x >> 16) as u8
            })
            .collect()
    }

    /// Which work unit of a strip an I/O is queued on.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum W {
        /// `wu`: the transfer's own.
        Wu,
        /// `wu_r`: the reads of a write, which run first.
        WuR,
    }

    /// One queued `sr_raid5_addio`.
    #[derive(Clone, Copy, Debug)]
    struct Op {
        wu: W,
        chunk: i64,
        blkno: Daddr,
        len: i64,
        data: usize,
        read: bool,
        freebuf: bool,
        xorbuf: Option<usize>,
    }

    /// A RAID 5 volume in memory: the chunks' bytes and states, an arena of buffers (a
    /// [`Raid5Io::Buf`] is an index into it; `None` once freed) and the queued I/O.
    struct Mem {
        chunks: Vec<Vec<u8>>,
        status: Vec<i32>,
        bufs: Vec<Option<Vec<u8>>>,
        ops: Vec<Op>,
        /// Fail the `addio` call with this index (counting down), to test the error paths.
        fail_in: Option<usize>,
    }

    impl Mem {
        /// `n` online, zeroed chunks of `rows` strips (a consistent volume: all parities 0).
        fn new(n: usize, rows: usize) -> Self {
            Mem {
                chunks: vec![vec![0; rows * STRIP as usize]; n],
                status: vec![BIOC_SDONLINE; n],
                bufs: Vec::new(),
                ops: Vec::new(),
                fail_in: None,
            }
        }

        fn rows(&self) -> usize {
            self.chunks[0].len() / STRIP as usize
        }

        /// The volume's data bytes.
        fn size(&self) -> usize {
            self.rows() * STRIP as usize * (self.chunks.len() - 1)
        }

        fn add(&mut self, b: Vec<u8>) -> usize {
            self.bufs.push(Some(b));
            self.bufs.len() - 1
        }

        fn take(&mut self, i: usize) -> Vec<u8> {
            self.bufs[i].take().expect("buffer already freed")
        }

        fn buf(&mut self, i: usize) -> &mut Vec<u8> {
            self.bufs[i].as_mut().expect("use of a freed buffer")
        }

        fn live(&self) -> usize {
            self.bufs.iter().filter(|b| b.is_some()).count()
        }

        /// Runs the queued I/O as the kernel does: the read work unit first, then the
        /// transfer's own (which it collides with).
        fn run(&mut self) {
            let ops = core::mem::take(&mut self.ops);
            for w in [W::WuR, W::Wu] {
                for op in ops.iter().filter(|o| o.wu == w) {
                    self.exec(op);
                }
            }
        }

        fn exec(&mut self, op: &Op) {
            let c = op.chunk as usize;
            let off = op.blkno as usize * DEV_BSIZE;
            let len = op.len as usize;
            if op.read {
                assert!(
                    matches!(self.status[c], BIOC_SDONLINE | BIOC_SDSCRUB),
                    "read of chunk {c} in state {}",
                    self.status[c]
                );
                let got = self.chunks[c][off..off + len].to_vec();
                self.buf(op.data)[..len].copy_from_slice(&got);
                if let Some(x) = op.xorbuf {
                    sr_raid5_xor(self.buf(x), &got, len);
                }
            } else {
                assert!(
                    matches!(
                        self.status[c],
                        BIOC_SDONLINE | BIOC_SDSCRUB | BIOC_SDREBUILD
                    ),
                    "write of chunk {c} in state {}",
                    self.status[c]
                );
                let src = self.buf(op.data)[..len].to_vec();
                self.chunks[c][off..off + len].copy_from_slice(&src);
            }
            if op.freebuf {
                self.bufs[op.data] = None;
            }
        }

        /// Drops the queued I/O unrun (a work unit put back after an error), freeing the blocks
        /// the ccbs own as a leak-free core would.
        fn discard(&mut self) {
            for op in core::mem::take(&mut self.ops) {
                if op.freebuf {
                    self.bufs[op.data] = None;
                }
            }
        }

        /// The bytes of strip-row `row` of chunk `c`.
        fn strip(&self, c: usize, row: usize) -> &[u8] {
            &self.chunks[c][row * STRIP as usize..(row + 1) * STRIP as usize]
        }

        /// Every row's strips XOR to zero (the parity is the XOR of the data).
        fn assert_parity(&self) {
            for row in 0..self.rows() {
                let mut x = vec![0u8; STRIP as usize];
                for c in 0..self.chunks.len() {
                    sr_raid5_xor(&mut x, self.strip(c, row), STRIP as usize);
                }
                assert!(x.iter().all(|&b| b == 0), "row {row} parity is wrong");
            }
        }
    }

    impl Raid5Io for Mem {
        type Wu = W;
        type Buf = usize;

        fn chunk_no(&self) -> i64 {
            self.chunks.len() as i64
        }

        fn chunk_status(&self, chunk: i64) -> i32 {
            self.status[chunk as usize]
        }

        fn addio(
            &mut self,
            wu: W,
            chunk: i64,
            blkno: Daddr,
            len: i64,
            data: Option<usize>,
            xsflags: i32,
            mut ccbflags: i32,
            xorbuf: Option<usize>,
        ) -> Result<(), Errno> {
            if let Some(n) = self.fail_in {
                if n == 0 {
                    // as sr_ccb_rw failing: a block handed over with FREEBUF goes back
                    if let Some(d) = data
                        && ccbflags & SR_CCBF_FREEBUF != 0
                    {
                        self.bufs[d] = None;
                    }
                    return Err(Errno::EIO);
                }
                self.fail_in = Some(n - 1);
            }
            let data = match data {
                Some(d) => d,
                None => {
                    ccbflags |= SR_CCBF_FREEBUF;
                    self.add(vec![0; len as usize])
                }
            };
            assert!(xorbuf.is_none() || data != xorbuf.unwrap_or(usize::MAX));
            self.ops.push(Op {
                wu,
                chunk,
                blkno,
                len,
                data,
                read: xsflags & SCSI_DATA_IN != 0,
                freebuf: ccbflags & SR_CCBF_FREEBUF != 0,
                xorbuf,
            });
            Ok(())
        }

        fn block_get(&mut self, len: i64) -> Option<usize> {
            Some(self.add(vec![0; len as usize]))
        }

        fn block_put(&mut self, buf: usize, _len: i64) {
            assert!(self.bufs[buf].take().is_some(), "double free");
        }

        fn zero(&mut self, buf: usize, len: i64) {
            self.buf(buf)[..len as usize].fill(0);
        }

        fn copy(&mut self, dst: usize, src: usize, len: i64) {
            let s = self.buf(src)[..len as usize].to_vec();
            self.buf(dst)[..len as usize].copy_from_slice(&s);
        }
    }

    /// A read or write of `data.len()` bytes at byte `offset` of the volume, strip by strip as
    /// `sr_raid5_rw` does it, each strip's I/O run before the next.
    fn vol_io(m: &mut Mem, write: bool, offset: usize, data: &mut [u8]) -> Result<(), Errno> {
        let no_chunk = m.chunk_no() - 1;
        let mut lbaoffs = offset as i64;
        let mut datalen = data.len() as i64;
        let mut done = 0usize;
        while datalen != 0 {
            let st = Raid5Strip::new(lbaoffs, datalen, STRIP, BITS, no_chunk);
            let len = st.length as usize;
            let piece = m.add(data[done..done + len].to_vec());
            let r = if write {
                sr_raid5_write(
                    m,
                    W::Wu,
                    W::WuR,
                    st.chunk,
                    st.parity,
                    st.lba,
                    st.length,
                    piece,
                    SCSI_DATA_OUT,
                    0,
                )
            } else {
                match m.chunk_status(st.chunk) {
                    BIOC_SDONLINE | BIOC_SDSCRUB => m.addio(
                        W::Wu,
                        st.chunk,
                        st.lba,
                        st.length,
                        Some(piece),
                        SCSI_DATA_IN,
                        0,
                        None,
                    ),
                    BIOC_SDOFFLINE | BIOC_SDREBUILD | BIOC_SDHOTSPARE => {
                        sr_raid5_regenerate(m, W::Wu, st.chunk, st.lba, st.length, piece)
                    }
                    _ => Err(Errno::EIO),
                }
            };
            if let Err(e) = r {
                m.discard();
                m.take(piece);
                return Err(e);
            }
            m.run();
            let got = m.take(piece);
            if !write {
                data[done..done + len].copy_from_slice(&got);
            }
            lbaoffs += st.length;
            datalen -= st.length;
            done += len;
        }
        Ok(())
    }

    /// Reads the whole volume and compares it with `expect`.
    fn assert_reads(m: &mut Mem, expect: &[u8]) {
        let mut got = vec![0u8; expect.len()];
        vol_io(m, false, 0, &mut got).unwrap();
        assert!(got == expect, "volume reads back wrong");
        assert_eq!(m.live(), 0, "leaked blocks");
    }

    #[test]
    fn strip_map_is_left_asymmetric() {
        for n in 3..=8i64 {
            let no_chunk = n - 1;
            for row in 0..3 * n {
                let mut seen = vec![false; n as usize];
                let mut last = -1;
                let mut parity = None;
                for k in 0..no_chunk {
                    let strip_no = row * no_chunk + k;
                    let st = Raid5Strip::new(strip_no << BITS, STRIP, STRIP, BITS, no_chunk);
                    assert_eq!(st.strip_no, strip_no);
                    assert_eq!(st.length, STRIP);
                    assert_eq!(st.lba, row * (STRIP / DEV_BSIZE as i64));
                    // parity starts on the last chunk and moves left one chunk per row
                    assert_eq!(st.parity, (n - 1) - row % n);
                    assert_eq!(*parity.get_or_insert(st.parity), st.parity);
                    // data fills the other chunks left to right
                    assert!(st.chunk > last && st.chunk != st.parity && st.chunk < n);
                    last = st.chunk;
                    seen[st.chunk as usize] = true;
                }
                seen[parity.unwrap() as usize] = true;
                assert!(seen.iter().all(|&s| s), "n {n} row {row}");
            }
        }
    }

    #[test]
    fn strip_map_partial_strips() {
        // 3 data chunks; a transfer starting 1024 bytes into strip 4: row 1, whose parity is on
        // chunk 3 - 1 = 2; strip 4 % 3 = 1 is left of it
        let st = Raid5Strip::new((4 << BITS) + 1024, 10_000, STRIP, BITS, 3);
        assert_eq!(st.strip_no, 4);
        assert_eq!(st.length, STRIP - 1024);
        assert_eq!(st.parity, 2);
        assert_eq!(st.chunk, 1);
        assert_eq!(st.lba, (STRIP + 1024) / DEV_BSIZE as i64);
        // strip 5 % 3 = 2 is the parity's chunk: it moves right of it
        let st = Raid5Strip::new(5 << BITS, 100, STRIP, BITS, 3);
        assert_eq!((st.chunk, st.parity, st.length), (3, 2, 100));
    }

    #[test]
    fn volume_size_and_openings() {
        let strip = MAXPHYS as u32;
        let blocks = i64::from(strip) / DEV_BSIZE as i64;
        assert_eq!(raid5_volume_size(10 * blocks + 7, strip, 3), 20 * blocks);
        assert_eq!(raid5_volume_size(10 * blocks, strip, 5), 40 * blocks);

        let _g = setup_real_memory();
        let sd = discipline(4, &[BIOC_SDONLINE; 4]);
        sr_raid5_discipline_init(sd);
        assert_eq!(sd.sd_type.get(), SR_MD_RAID5);
        assert_eq!(&sd.sd_name.get()[..6], b"RAID 5");
        assert_eq!(sd.sd_max_wu.get(), SR_RAID5_NOWU + 2);
        assert_eq!(sr_raid5_openings(sd), SR_RAID5_NOWU as i32 / 2);
        assert!(sd.sd_capabilities.get() & SR_CAP_REBUILD != 0);
        sd.sd_meta().ssdi().ssd_strip_size.set(strip);
        sr_raid5_init(sd).unwrap();
        assert_eq!(
            sd.mds().mdd_raid5.sr5_strip_bits.get(),
            strip.trailing_zeros() as i32
        );
        assert_eq!(sd.sd_max_ccb_per_wu.get(), 4);
    }

    #[test]
    fn xor_works_in_words() {
        let mut a = vec![0x0fu8; 7];
        sr_raid5_xor(&mut a, &[0xffu8; 7], 7);
        // 7 & !3 = 4 bytes: the C's one 32-bit word
        assert_eq!(a, [0xf0, 0xf0, 0xf0, 0xf0, 0x0f, 0x0f, 0x0f]);
    }

    #[test]
    fn chunk_state_table() {
        let ok = [
            (BIOC_SDONLINE, BIOC_SDOFFLINE),
            (BIOC_SDONLINE, BIOC_SDSCRUB),
            (BIOC_SDOFFLINE, BIOC_SDREBUILD),
            (BIOC_SDSCRUB, BIOC_SDONLINE),
            (BIOC_SDSCRUB, BIOC_SDOFFLINE),
            (BIOC_SDREBUILD, BIOC_SDONLINE),
            (BIOC_SDREBUILD, BIOC_SDOFFLINE),
        ];
        for old in 0..SR_MAX_STATES as i32 {
            for new in 0..SR_MAX_STATES as i32 {
                if old != new {
                    assert_eq!(
                        raid5_chunk_state_ok(old, new),
                        ok.contains(&(old, new)),
                        "{old} -> {new}"
                    );
                }
            }
        }
    }

    #[test]
    fn volume_state_from_chunks() {
        let st = |online: i64, scrub: i64, rebuild: i64, offline: i64| {
            let mut s = [0i64; SR_MAX_STATES];
            s[BIOC_SDONLINE as usize] = online;
            s[BIOC_SDSCRUB as usize] = scrub;
            s[BIOC_SDREBUILD as usize] = rebuild;
            s[BIOC_SDOFFLINE as usize] = offline;
            raid5_vol_state(&s, online + scrub + rebuild + offline)
        };
        assert_eq!(st(4, 0, 0, 0), Some(BIOC_SVONLINE));
        assert_eq!(st(3, 0, 0, 1), Some(BIOC_SVDEGRADED));
        assert_eq!(st(3, 0, 1, 0), Some(BIOC_SVREBUILD));
        assert_eq!(st(3, 1, 0, 0), Some(BIOC_SVSCRUB));
        assert_eq!(st(2, 0, 0, 2), Some(BIOC_SVOFFLINE));
        assert_eq!(st(2, 0, 1, 1), Some(BIOC_SVOFFLINE));

        assert!(raid5_vol_state_ok(BIOC_SVONLINE, BIOC_SVDEGRADED));
        assert!(raid5_vol_state_ok(BIOC_SVONLINE, BIOC_SVREBUILD));
        assert!(!raid5_vol_state_ok(BIOC_SVONLINE, BIOC_SVSCRUB));
        assert!(raid5_vol_state_ok(BIOC_SVDEGRADED, BIOC_SVREBUILD));
        assert!(!raid5_vol_state_ok(BIOC_SVDEGRADED, BIOC_SVONLINE));
        assert!(raid5_vol_state_ok(BIOC_SVREBUILD, BIOC_SVONLINE));
        assert!(raid5_vol_state_ok(BIOC_SVBUILDING, BIOC_SVBUILDING));
        assert!(!raid5_vol_state_ok(BIOC_SVOFFLINE, BIOC_SVOFFLINE));
        assert!(!raid5_vol_state_ok(BIOC_SVOFFLINE, BIOC_SVONLINE));
    }

    /// A discipline over `n` chunks in the states `status`, as the softraid tests make one.
    fn discipline(n: usize, status: &[i32]) -> &'static SrDiscipline {
        // SAFETY: zeroed (`SrZeroed`) and leaked.
        let sd: &'static SrDiscipline =
            unsafe { sr_malloc::<SrDiscipline>(M_WAITOK).unwrap().as_ref() };
        // SAFETY: a zeroed softc (a `Softc`: all-zero bytes are a valid value), leaked.
        let sc: &'static SrSoftc = std::boxed::Box::leak(std::boxed::Box::new(unsafe {
            core::mem::zeroed::<SrSoftc>()
        }));
        sd.sd_sc.set(sc);
        sd.sd_meta.set(Some(
            sr_malloc_size::<SrMetadata>(crate::dev::softraid::SR_META_BYTES, M_WAITOK).unwrap(),
        ));
        sd.sd_meta().ssdi().ssd_chunk_no.set(n as u32);
        sd.sd_vol.sv_chunks_alloc(n, M_WAITOK).unwrap();
        for (i, &s) in status.iter().enumerate() {
            // SAFETY: zeroed (`SrZeroed`) and leaked.
            let c: &'static SrChunk = unsafe { sr_malloc::<SrChunk>(M_WAITOK).unwrap().as_ref() };
            c.src_meta.scm_status.set(s as u32);
            sd.sd_vol.set_sv_chunk(i, Some(c));
        }
        sd
    }

    #[test]
    fn set_vol_state_follows_chunks() {
        let _g = setup_real_memory();
        let sd = discipline(4, &[BIOC_SDONLINE; 4]);
        sd.sd_vol_status.set(BIOC_SVONLINE);
        sr_raid5_set_vol_state(sd);
        assert_eq!(sd.sd_vol_status.get(), BIOC_SVONLINE);
        sd.sd_vol
            .sv_chunk(2)
            .src_meta
            .scm_status
            .set(BIOC_SDOFFLINE as u32);
        sr_raid5_set_vol_state(sd);
        assert_eq!(sd.sd_vol_status.get(), BIOC_SVDEGRADED);
        sd.sd_vol
            .sv_chunk(2)
            .src_meta
            .scm_status
            .set(BIOC_SDREBUILD as u32);
        sr_raid5_set_vol_state(sd);
        assert_eq!(sd.sd_vol_status.get(), BIOC_SVREBUILD);
        sd.sd_vol
            .sv_chunk(0)
            .src_meta
            .scm_status
            .set(BIOC_SDOFFLINE as u32);
        sr_raid5_set_vol_state(sd);
        assert_eq!(sd.sd_vol_status.get(), BIOC_SVOFFLINE);
    }

    #[test]
    fn writes_keep_parity_and_read_back() {
        for n in 3..=6 {
            let mut m = Mem::new(n, 2 * n);
            let mut expect = vec![0u8; m.size()];
            // the whole volume, then pieces (whole sectors, as transfers are) that cross strips
            let mut all = bytes(n as u32, m.size());
            vol_io(&mut m, true, 0, &mut all).unwrap();
            expect.copy_from_slice(&all);
            for (k, (off, len)) in [(512, 9216), (4096 * 3 - 512, 1024), (1536, 512)]
                .iter()
                .enumerate()
            {
                let mut piece = bytes(1000 + k as u32, *len);
                vol_io(&mut m, true, *off, &mut piece).unwrap();
                expect[*off..*off + *len].copy_from_slice(&piece);
            }
            assert_eq!(m.live(), 0, "leaked blocks");
            m.assert_parity();
            assert_reads(&mut m, &expect);
        }
    }

    #[test]
    fn any_one_missing_chunk_is_regenerated() {
        for n in 3..=6 {
            let mut m = Mem::new(n, 2 * n);
            let expect = bytes(7 * n as u32, m.size());
            vol_io(&mut m, true, 0, &mut expect.clone()).unwrap();
            for c in 0..n {
                for state in [BIOC_SDOFFLINE, BIOC_SDREBUILD, BIOC_SDHOTSPARE] {
                    m.status[c] = state;
                    assert_reads(&mut m, &expect);
                }
                m.status[c] = BIOC_SDONLINE;
            }
            // two missing chunks: a strip on either cannot be regenerated
            m.status[0] = BIOC_SDOFFLINE;
            m.status[1] = BIOC_SDOFFLINE;
            let mut got = vec![0u8; expect.len()];
            assert_eq!(vol_io(&mut m, false, 0, &mut got), Err(Errno::EIO));
            assert_eq!(m.live(), 0, "leaked blocks");
        }
    }

    #[test]
    fn degraded_writes_cover_the_four_cases() {
        // With one chunk offline, each row is case 2 (it holds the row's parity), case 3 (it
        // holds the strip) or case 4 (another strip of the row).
        for n in 3..=6 {
            for off in 0..n {
                let mut m = Mem::new(n, 2 * n);
                let mut expect = bytes(3 * n as u32, m.size());
                vol_io(&mut m, true, 0, &mut expect.clone()).unwrap();
                m.status[off] = BIOC_SDOFFLINE;
                let mut new = bytes(100 + off as u32, m.size());
                vol_io(&mut m, true, 0, &mut new).unwrap();
                expect.copy_from_slice(&new);
                assert_eq!(m.live(), 0, "leaked blocks");
                // the missing chunk's strips come back from the new parity
                assert_reads(&mut m, &expect);
            }
        }
    }

    #[test]
    fn writes_onto_a_rebuilding_chunk() {
        // The rebuilding chunk is written as normal; the other strips of its rows update the
        // parity from the old data and parity (case 4), so after a full write the chunk holds
        // its data and every row's parity is right.
        for n in 3..=5 {
            for rc in 0..n {
                let mut m = Mem::new(n, 2 * n);
                m.status[rc] = BIOC_SDREBUILD;
                let expect = bytes(11 * n as u32 + rc as u32, m.size());
                vol_io(&mut m, true, 0, &mut expect.clone()).unwrap();
                m.status[rc] = BIOC_SDONLINE;
                m.assert_parity();
                assert_reads(&mut m, &expect);
            }
        }
    }

    #[test]
    fn rebuild_strips_regenerate_the_chunk() {
        // sr_raid5_rebuild's loop body: regenerate each strip of the chunk from the others into a
        // block, write the block to the chunk.
        let n = 5;
        let mut m = Mem::new(n, 8);
        let mut data = bytes(42, m.size());
        vol_io(&mut m, true, 0, &mut data).unwrap();
        for rc in 0..n {
            let orig = m.chunks[rc].clone();
            m.chunks[rc].fill(0xa5);
            m.status[rc] = BIOC_SDREBUILD;
            for strip_no in 0..m.rows() as i64 {
                let chunk_lba = (STRIP >> DEV_BSHIFT) * strip_no;
                let xorbuf = m.block_get(STRIP).unwrap();
                sr_raid5_regenerate(&mut m, W::WuR, rc as i64, chunk_lba, STRIP, xorbuf).unwrap();
                m.addio(
                    W::Wu,
                    rc as i64,
                    chunk_lba,
                    STRIP,
                    Some(xorbuf),
                    SCSI_DATA_OUT,
                    SR_CCBF_FREEBUF,
                    None,
                )
                .unwrap();
                m.run();
            }
            m.status[rc] = BIOC_SDONLINE;
            assert!(m.chunks[rc] == orig, "chunk {rc} not rebuilt");
            assert_eq!(m.live(), 0, "leaked blocks");
        }
    }

    #[test]
    fn failed_queueing_frees_the_parity_block() {
        // Fail each addio of a write of strip 0 (chunk 0, parity on 3) in turn, all online
        // (case 1) and with chunk 1 offline (case 4): the write fails and no block is left
        // behind; four calls each (two reads, the parity, the data).
        for off in [None, Some(1)] {
            for k in 0..6 {
                let mut m = Mem::new(4, 4);
                if let Some(o) = off {
                    m.status[o] = BIOC_SDOFFLINE;
                }
                m.fail_in = Some(k);
                let mut piece = bytes(k as u32, STRIP as usize);
                let r = vol_io(&mut m, true, 0, &mut piece);
                assert_eq!(r.is_ok(), k >= 4, "addio {k} failed");
                assert_eq!(m.live(), 0, "leaked blocks (addio {k} failed)");
            }
        }
    }
}
/* </TESTS> */
