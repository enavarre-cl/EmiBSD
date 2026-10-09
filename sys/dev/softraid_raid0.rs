/* <LICENSES> */
/*
 * Copyright (c) 2008 Marco Peereboom <marco@peereboom.us>
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
//! `softraid_raid0.c`: the RAID 0 (striping) discipline of softraid(4). A volume's blocks
//! are laid out in strips of `ssd_strip_size` bytes (`MAXPHYS`) dealt round-robin over the
//! chunks; a read or write is split into one ccb per strip it touches, all of which must be
//! on online chunks (RAID 0 has no redundancy).
//!
//! Upstream: sys/dev/softraid_raid0.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The strip walk of `sr_raid0_rw` (which chunk, which offset on it and how many bytes each
//!   ccb gets) is [`Raid0Walk`], a plain value the host tests drive; `sr_raid0_rw` takes the
//!   same steps in the same order as the C's `for (;;)` loop, including its asymmetric offset
//!   update (`offset -= stripoffs` only after the first ccb).
//! - The C's `return (1)` is `Err(Errno::EIO)` (`softraidvar.rs`); `sr_raid0_create`'s
//!   `EINVAL` is kept.
//! - `DNPRINTF` calls are comments (`SR_DEBUG` is not configured).

use crate::dev::biovar::{BIOC_SDONLINE, BiocCreateraid};
use crate::dev::softraid::{
    sr_ccb_rw, sr_error, sr_schedule_wu, sr_validate_io, sr_validate_stripsize, sr_wu_enqueue_ccb,
};
use crate::dev::softraidvar::*;
use crate::kern::subr_prf::printf;
use crate::sys::errno::Errno;
use crate::sys::param::{DEV_BSHIFT, MAXPHYS};
use crate::sys::types::Daddr;

/// `sr_raid0_discipline_init`: discipline initialisation.
pub fn sr_raid0_discipline_init(sd: &'static SrDiscipline) {
    // Fill out discipline members.
    sd.sd_type.set(SR_MD_RAID0);
    sd.sd_name.set(*b"RAID 0\0\0\0\0");
    sd.sd_capabilities
        .set(SR_CAP_SYSTEM_DISK | SR_CAP_AUTO_ASSEMBLE);
    sd.sd_max_wu.set(SR_RAID0_NOWU);

    // Setup discipline specific function pointers.
    sd.sd_assemble.set(Some(sr_raid0_assemble));
    sd.sd_create.set(Some(sr_raid0_create));
    sd.sd_scsi_rw.set(Some(sr_raid0_rw));
}

/// `sr_raid0_create`: sets up the metadata of a new RAID 0 volume of `no_chunk` chunks of
/// `coerced_size` blocks.
pub fn sr_raid0_create(
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

    // XXX add variable strip size later even though MAXPHYS is really
    // the clever value, users like to tinker with that type of stuff.
    let ssdi = sd.sd_meta().ssdi();
    ssdi.ssd_strip_size.set(MAXPHYS as u32);
    ssdi.ssd_size
        .set(raid0_volume_size(coerced_size, MAXPHYS as u32, no_chunk));

    sr_raid0_init(sd)
}

/// The size in blocks of a RAID 0 volume of `no_chunk` chunks of `coerced_size` blocks: the
/// chunk size truncated to a whole number of strips, times the number of chunks.
fn raid0_volume_size(coerced_size: i64, strip_size: u32, no_chunk: i32) -> i64 {
    let strip_blocks = u64::from(strip_size) >> DEV_BSHIFT;
    ((coerced_size as u64 & !(strip_blocks - 1)).wrapping_mul(no_chunk as u64)) as i64
}

/// `sr_raid0_assemble`: brings up an existing RAID 0 volume.
pub fn sr_raid0_assemble(
    sd: &'static SrDiscipline,
    _bc: &mut BiocCreateraid,
    _no_chunks: i32,
    _data: Option<&[u8]>,
) -> Result<(), Errno> {
    sr_raid0_init(sd)
}

/// `sr_raid0_init`: initialises the runtime values (the strip shift and the ccb budget) from
/// the metadata.
pub fn sr_raid0_init(sd: &'static SrDiscipline) -> Result<(), Errno> {
    let ssdi = sd.sd_meta().ssdi();

    // Initialise runtime values.
    let strip_bits = sr_validate_stripsize(ssdi.ssd_strip_size.get()).unwrap_or(-1);
    sd.mds().mdd_raid0.sr0_strip_bits.set(strip_bits);
    if strip_bits == -1 {
        sr_error(
            sd.sd_sc(),
            format_args!("{}: invalid strip size", sd.name()),
        );
        return Err(Errno::EINVAL);
    }
    sd.sd_max_ccb_per_wu.set(
        (MAXPHYS as u32 / ssdi.ssd_strip_size.get() + 1) * SR_RAID0_NOWU * ssdi.ssd_chunk_no.get(),
    );

    Ok(())
}

/// The strips a transfer touches, one [`Raid0Walk::length`] bytes at a time: the C's
/// `chunk`, `offset`, `length` and `leftover` of `sr_raid0_rw`, with their updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Raid0Walk {
    /// `strip_size`.
    strip_size: i64,
    /// `no_chunk`.
    no_chunk: i64,
    /// `stripoffs`: the offset of the transfer in its first strip.
    stripoffs: i64,
    /// `chunk`: the chunk of the current piece.
    chunk: i64,
    /// `offset`: its byte offset on that chunk (relative to the data area).
    offset: i64,
    /// `length`: its length in bytes.
    length: i64,
    /// `leftover`: the bytes of the transfer still to do, this piece included.
    leftover: i64,
}

impl Raid0Walk {
    /// The walk of `datalen` bytes at block `blkno` of the volume.
    fn new(blkno: Daddr, datalen: i64, strip_size: i64, strip_bits: i64, no_chunk: i64) -> Self {
        // all offs are in bytes
        let lbaoffs = blkno << DEV_BSHIFT;
        let strip_no = lbaoffs >> strip_bits;
        let chunk = strip_no % no_chunk;
        let stripoffs = lbaoffs & (strip_size - 1);
        let chunkoffs = (strip_no / no_chunk) << strip_bits;
        Raid0Walk {
            strip_size,
            no_chunk,
            stripoffs,
            chunk,
            offset: chunkoffs + stripoffs,
            length: datalen.min(strip_size - stripoffs),
            leftover: datalen,
        }
    }

    /// The block of the current piece on its chunk (relative to the data area).
    fn blkno(&self) -> Daddr {
        self.offset >> DEV_BSHIFT
    }

    /// Finishes the current piece and moves to the next; `false` when it was the last.
    /// `first` is the C's `wu->swu_io_count == 1`: the piece was the transfer's first ccb.
    fn advance(&mut self, first: bool) -> bool {
        self.leftover -= self.length;
        if self.leftover == 0 {
            return false;
        }

        self.chunk += 1;
        if self.chunk > self.no_chunk - 1 {
            self.chunk = 0;
            self.offset += self.length;
        } else if first {
            self.offset -= self.stripoffs;
        }
        self.length = self.leftover.min(self.strip_size);

        true
    }
}

/// `sr_raid0_rw`: splits a read or write into one ccb per strip it touches and schedules
/// the work unit.
pub fn sr_raid0_rw(wu: &'static SrWorkunit) -> Result<(), Errno> {
    let sd = wu.dis();
    let xs = wu.xs();

    // blkno and scsi error will be handled by sr_validate_io
    // (the work unit is unwound by sr_wu_put)
    let blkno = sr_validate_io(wu, "sr_raid0_rw")?;

    let strip_size = i64::from(sd.sd_meta().ssdi().ssd_strip_size.get());
    let strip_bits = i64::from(sd.mds().mdd_raid0.sr0_strip_bits.get());
    let no_chunk = i64::from(sd.sd_meta().ssdi().ssd_chunk_no.get());

    // DNPRINTF(SR_D_DIS, "%s: %s: front end io: blkno %lld size %d")

    let mut walk = Raid0Walk::new(
        blkno,
        i64::from(xs.datalen()),
        strip_size,
        strip_bits,
        no_chunk,
    );
    let mut done: usize = 0;
    loop {
        // make sure chunk is online
        let scp = sd.sd_vol.sv_chunk(walk.chunk as usize);
        if scp.src_meta.scm_status.get() != BIOC_SDONLINE as u32 {
            return Err(Errno::EIO);
        }

        // DNPRINTF(SR_D_DIS, "%s: %s %s io lbaoffs %lld strip_no %lld chunk %lld ...")

        // SAFETY: `xs.data()` is valid for `datalen` bytes until the transfer completes
        // (`ScsiXfer::set_data`), and this piece lies inside it: the pieces are consecutive
        // and add up to `datalen`.
        let ccb = unsafe {
            sr_ccb_rw(
                sd,
                walk.chunk as usize,
                walk.blkno(),
                walk.length,
                xs.data().wrapping_add(done),
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

        let length = walk.length as usize;
        if !walk.advance(wu.swu_io_count.get() == 1) {
            break;
        }
        done += length;
    }

    sr_schedule_wu(wu);

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
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
}
/* </TESTS> */
