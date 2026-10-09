/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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

/*
 * Copyright (c) 2008 Marco Peereboom <marco@peereboom.us>
 * Copyright (c) 2011 Joel Sing <jsing@openbsd.org>
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
//! `softraid_concat.c`: the CONCAT discipline of softraid(4). The volume is its chunks one
//! after the other, each at its own size (`SR_CAP_NON_COERCED`); a transfer is split at the
//! chunk boundaries (and at `MAXPHYS`) into one ccb per piece, all of which must be on
//! online chunks (CONCAT has no redundancy).
//!
//! Upstream: sys/dev/softraid_concat.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The search for the chunk holding a byte offset of the volume is [`concat_locate`], a
//!   function of the chunk sizes the host tests drive. The C also walks past the last chunk
//!   when the offset is exactly the volume's size (`sr_validate_io` lets the block after the
//!   last one through) and reads `sv_chunks[no_chunk]`; here that, like an offset beyond the
//!   end, fails the work unit with `EIO` instead.
//! - The C's `return (1)` is `Err(Errno::EIO)` (`softraidvar.rs`); `sr_concat_create`'s
//!   `EINVAL` is kept.
//! - `DNPRINTF` calls are comments (`SR_DEBUG` is not configured).

use crate::dev::biovar::{BIOC_SDONLINE, BiocCreateraid};
use crate::dev::softraid::{
    sr_ccb_rw, sr_error, sr_schedule_wu, sr_validate_io, sr_wu_enqueue_ccb,
};
use crate::dev::softraidvar::*;
use crate::kern::subr_prf::printf;
use crate::sys::errno::Errno;
use crate::sys::param::{DEV_BSHIFT, MAXPHYS};

/// `sr_concat_discipline_init`: discipline initialisation.
pub fn sr_concat_discipline_init(sd: &'static SrDiscipline) {
    // Fill out discipline members.
    sd.sd_type.set(SR_MD_CONCAT);
    sd.sd_name.set(*b"CONCAT\0\0\0\0");
    sd.sd_capabilities
        .set(SR_CAP_SYSTEM_DISK | SR_CAP_AUTO_ASSEMBLE | SR_CAP_NON_COERCED);
    sd.sd_max_wu.set(SR_CONCAT_NOWU);

    // Setup discipline specific function pointers.
    sd.sd_assemble.set(Some(sr_concat_assemble));
    sd.sd_create.set(Some(sr_concat_create));
    sd.sd_scsi_rw.set(Some(sr_concat_rw));
}

/// `sr_concat_create`: sets up the metadata of a new CONCAT volume of `no_chunk` chunks (its
/// size is the sum of theirs).
pub fn sr_concat_create(
    sd: &'static SrDiscipline,
    _bc: &mut BiocCreateraid,
    no_chunk: i32,
    _coerced_size: i64,
) -> Result<(), Errno> {
    if no_chunk < 1 {
        sr_error(
            sd.sd_sc(),
            format_args!("{} requires one or more chunks", sd.name()),
        );
        return Err(Errno::EINVAL);
    }

    let ssdi = sd.sd_meta().ssdi();
    ssdi.ssd_size.set(0);
    for i in 0..no_chunk as usize {
        ssdi.ssd_size
            .set(ssdi.ssd_size.get() + sd.sd_vol.sv_chunk(i).src_size.get());
    }

    sr_concat_init(sd)
}

/// `sr_concat_assemble`: brings up an existing CONCAT volume.
pub fn sr_concat_assemble(
    sd: &'static SrDiscipline,
    _bc: &mut BiocCreateraid,
    _no_chunk: i32,
    _data: Option<&[u8]>,
) -> Result<(), Errno> {
    sr_concat_init(sd)
}

/// `sr_concat_init`: initialises the runtime values (the ccb budget) from the metadata.
pub fn sr_concat_init(sd: &'static SrDiscipline) -> Result<(), Errno> {
    sd.sd_max_ccb_per_wu
        .set(SR_CONCAT_NOWU * sd.sd_meta().ssdi().ssd_chunk_no.get());

    Ok(())
}

/// Where byte `lbaoffs` of a CONCAT volume lives: the chunk (an index into `sizes`, the
/// chunks' sizes in blocks), the byte offset of the end of that chunk in the volume
/// (`chunkend`) and `lbaoffs`'s offset on the chunk; `None` when `lbaoffs` is not inside
/// any chunk.
fn concat_locate(sizes: impl Iterator<Item = i64>, lbaoffs: i64) -> Option<(usize, i64, i64)> {
    let mut chunkend = 0;
    let mut offset = lbaoffs;
    for (chunk, size) in sizes.enumerate() {
        let chunksize = size << DEV_BSHIFT;
        chunkend += chunksize;
        if lbaoffs < chunkend {
            return Some((chunk, chunkend, offset));
        }
        offset -= chunksize;
    }

    None
}

/// `sr_concat_rw`: splits a read or write at the chunk boundaries into one ccb per piece and
/// schedules the work unit.
pub fn sr_concat_rw(wu: &'static SrWorkunit) -> Result<(), Errno> {
    let sd = wu.dis();
    let xs = wu.xs();

    // blkno and scsi error will be handled by sr_validate_io
    // (the work unit is unwound by sr_wu_put)
    let blkno = sr_validate_io(wu, "sr_concat_rw")?;

    let no_chunk = sd.sd_meta().ssdi().ssd_chunk_no.get() as usize;

    // DNPRINTF(SR_D_DIS, "%s: %s: front end io: blkno %lld size %d")

    // All offsets are in bytes.
    let mut lbaoffs = blkno << DEV_BSHIFT;
    let mut leftover = i64::from(xs.datalen());
    let mut done: usize = 0;
    loop {
        let sizes = (0..no_chunk).map(|chunk| sd.sd_vol.sv_chunk(chunk).src_size.get());
        let Some((chunk, chunkend, offset)) = concat_locate(sizes, lbaoffs) else {
            return Err(Errno::EIO);
        };

        let length = leftover.min(chunkend - lbaoffs).min(MAXPHYS as i64);

        // make sure chunk is online
        let scp = sd.sd_vol.sv_chunk(chunk);
        if scp.src_meta.scm_status.get() != BIOC_SDONLINE as u32 {
            return Err(Errno::EIO);
        }

        // DNPRINTF(SR_D_DIS, "%s: %s %s io lbaoffs %lld chunk %lld chunkend %lld ...")

        // SAFETY: `xs.data()` is valid for `datalen` bytes until the transfer completes
        // (`ScsiXfer::set_data`), and this piece lies inside it: the pieces are consecutive
        // and add up to `datalen`.
        let ccb = unsafe {
            sr_ccb_rw(
                sd,
                chunk,
                offset >> DEV_BSHIFT,
                length,
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

        leftover -= length;
        if leftover == 0 {
            break;
        }
        done += length as usize;
        lbaoffs += length;
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

    use crate::dev::softraid::SR_META_BYTES;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::sys::malloc::M_WAITOK;

    /// A discipline with in-memory metadata and chunks of the given sizes (in blocks).
    fn volume(sizes: &[i64]) -> &'static SrDiscipline {
        // SAFETY: `SrSoftc` is a `Softc`: all-zero bytes are a valid value of it.
        let sc: &'static SrSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed::<SrSoftc>() }));
        let sd = sr_malloc::<SrDiscipline>(M_WAITOK).unwrap();
        // SAFETY: a zeroed discipline (`SrZeroed`), never freed in the tests.
        let sd: &'static SrDiscipline = unsafe { sd.as_ref() };
        sd.sd_sc.set(sc);
        sd.sd_meta.set(Some(
            sr_malloc_size::<SrMetadata>(SR_META_BYTES, M_WAITOK).unwrap(),
        ));
        sd.sd_meta().ssdi().ssd_chunk_no.set(sizes.len() as u32);
        sd.sd_vol.sv_chunks_alloc(sizes.len(), M_WAITOK).unwrap();
        for (i, &size) in sizes.iter().enumerate() {
            // SAFETY: a zeroed chunk (`SrZeroed`), leaked.
            let c: &'static SrChunk = unsafe { sr_malloc::<SrChunk>(M_WAITOK).unwrap().as_ref() };
            c.src_size.set(size);
            sd.sd_vol.set_sv_chunk(i, Some(c));
        }
        sd
    }

    #[test]
    fn locate_walks_the_chunks() {
        let sizes = [10i64, 20, 5];
        let at = |lba: i64| concat_locate(sizes.iter().copied(), lba);
        // chunk ends in bytes: 5120, 15360, 17920
        assert_eq!(at(0), Some((0, 5120, 0)));
        assert_eq!(at(5119), Some((0, 5120, 5119)));
        // the first byte of the second chunk is its offset 0
        assert_eq!(at(5120), Some((1, 15360, 0)));
        assert_eq!(at(15359), Some((1, 15360, 10239)));
        assert_eq!(at(15360), Some((2, 17920, 0)));
        assert_eq!(at(17919), Some((2, 17920, 2559)));
        // at the end of the volume, and beyond it, there is no chunk
        assert_eq!(at(17920), None);
        assert_eq!(at(1 << 40), None);
        assert_eq!(concat_locate(core::iter::empty(), 0), None);
    }

    #[test]
    fn create_sums_the_chunks() {
        let _g = setup_real_memory();
        let sd = volume(&[1000, 2500, 7]);
        // SAFETY: a plain C structure of integers and null pointers: all-zero bytes are a value.
        let mut bc: BiocCreateraid = unsafe { core::mem::zeroed() };
        sr_concat_create(sd, &mut bc, 3, 7).unwrap();
        assert_eq!(sd.sd_meta().ssdi().ssd_size.get(), 3507);
        assert_eq!(sd.sd_max_ccb_per_wu.get(), SR_CONCAT_NOWU * 3);

        // assembly only sets the runtime values
        let sd = volume(&[1, 1]);
        sr_concat_assemble(sd, &mut bc, 2, None).unwrap();
        assert_eq!(sd.sd_max_ccb_per_wu.get(), SR_CONCAT_NOWU * 2);
    }

    #[test]
    fn discipline_init_installs_hooks() {
        let _g = setup_real_memory();
        let sd = volume(&[1]);
        sr_concat_discipline_init(sd);
        assert_eq!(sd.sd_type.get(), SR_MD_CONCAT);
        assert_eq!(sd.name().to_string(), "CONCAT");
        assert_eq!(
            sd.sd_capabilities.get(),
            SR_CAP_SYSTEM_DISK | SR_CAP_AUTO_ASSEMBLE | SR_CAP_NON_COERCED
        );
        assert_eq!(sd.sd_max_wu.get(), SR_CONCAT_NOWU);
        assert!(sd.sd_create.get().is_some());
        assert!(sd.sd_assemble.get().is_some());
        assert!(sd.sd_scsi_rw.get().is_some());
    }
}
/* </TESTS> */
