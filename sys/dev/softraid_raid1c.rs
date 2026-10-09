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
 * Copyright (c) 2007 Marco Peereboom <marco@peereboom.us>
 * Copyright (c) 2008 Hans-Joerg Hoexer <hshoexer@openbsd.org>
 * Copyright (c) 2008 Damien Miller <djm@mindrot.org>
 * Copyright (c) 2009 Joel Sing <jsing@openbsd.org>
 * Copyright (c) 2020 Stefan Sperling <stsp@openbsd.org>
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
//! `softraid_raid1c.c`: the RAID 1C discipline of softraid(4), RAID 1 with the CRYPTO
//! discipline's encryption: every chunk holds the same AES-XTS encrypted data. Writes are
//! encrypted once into the work unit's buffer, which is written to every working chunk;
//! reads are interleaved over the chunks and decrypted when done. The chunk and volume
//! state machines and the completion of work units are RAID 1's (`softraid_raid1.rs`); the
//! keys, metadata and ioctls are CRYPTO's (`softraid_crypto.rs`), on the `sr1c_crypto` half
//! of the discipline's `mdd_raid1c`.
//!
//! Upstream: sys/dev/softraid_raid1c.c @ 3ce1f3f79392
//!
//! A rebuild (`SR_WUF_REBUILD`) copies the encrypted sectors from a working chunk to the
//! one being rebuilt: its writes are not encrypted again and its reads not decrypted.
//!
//! ## Deviations
//! - The work units are `softraid_crypto.rs`'s [`SrCryptoWuReq`] (`struct sr_crypto_wu`,
//!   see that file).
//! - `sr_raid1c_rw`: a failed encryption fails the transfer by the error return alone, once,
//!   and writes nothing (the C completes it, writes anyway, and returns the error: see
//!   `softraid_crypto.rs`).
//! - The choice of the chunk a read goes to and what a write does to a chunk are pure
//!   functions ([`raid1c_read_chunk`], [`raid1c_write_action`]) that the host tests walk
//!   through; RAID 1's helpers are private to `softraid_raid1.rs`, and RAID 1C's write rule
//!   differs anyway (a rebuild skips the chunks that are online).
//! - `sr_raid1c_add_offline_chunks` with no chunk present (`no_chunk == 0`), where the C
//!   reads `chunks[-1]`, is `EINVAL`. The old `sv_chunks` array is freed by
//!   `SrVolume::sv_chunks_alloc`, which knows its size (the C passes `no_chunk` pointers).
//! - Hooks return `Result<(), Errno>`, the C's 1 being `Err(EIO)`; `DNPRINTF` calls are
//!   comments.

use alloc::vec::Vec;

use crate::crypto::crypto::crypto_invoke;
use crate::dev::biovar::{
    BIOC_SDHOTSPARE, BIOC_SDOFFLINE, BIOC_SDONLINE, BIOC_SDREBUILD, BIOC_SDSCRUB, BiocCreateraid,
    BiocDiscipline,
};
use crate::dev::softraid::{
    sr_ccb_rw, sr_error, sr_schedule_wu, sr_strlcpy_cell, sr_validate_io, sr_wu_enqueue_ccb,
};
use crate::dev::softraid_crypto::{
    SrCryptoWuReq, ccb_write_crypted, sr_crypto_alloc_resources_internal, sr_crypto_done_internal,
    sr_crypto_free_resources_internal, sr_crypto_ioctl_internal, sr_crypto_meta_create,
    sr_crypto_meta_opt_handler_internal, sr_crypto_prepare, sr_crypto_set_key,
};
use crate::dev::softraid_raid1::{
    sr_raid1_assemble, sr_raid1_init, sr_raid1_set_chunk_state, sr_raid1_set_vol_state,
    sr_raid1_wu_done,
};
use crate::dev::softraidvar::*;
use crate::kern::subr_prf::printf;
use crate::scsi::scsiconf::{SCSI_DATA_IN, SCSI_DATA_OUT};
use crate::sys::errno::Errno;
use crate::sys::malloc::M_WAITOK;
use crate::sys::param::NODEV;
use crate::sys::queue::SlistHead;

/// What a RAID 1C write does to a chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Raid1cWrite {
    /// Write to it.
    Write,
    /// Skip it (`continue`).
    Skip,
    /// A state no volume should be in: fail the work unit.
    Bad,
}

/// `sr_raid1c_discipline_init`: discipline initialisation.
pub fn sr_raid1c_discipline_init(sd: &'static SrDiscipline) {
    // Fill out discipline members.
    sd.set_wu_type::<SrCryptoWuReq>();
    sd.sd_type.set(SR_MD_RAID1C);
    sr_strlcpy_cell(&sd.sd_name, b"RAID 1C");
    sd.sd_capabilities
        .set(SR_CAP_SYSTEM_DISK | SR_CAP_AUTO_ASSEMBLE | SR_CAP_REBUILD | SR_CAP_REDUNDANT);
    sd.sd_max_wu.set(SR_RAID1C_NOWU);

    for sid in &sd.mds().mdd_raid1c.sr1c_crypto.scr_sid {
        sid.set(u64::MAX);
    }

    // Setup discipline specific function pointers.
    sd.sd_alloc_resources.set(Some(sr_raid1c_alloc_resources));
    sd.sd_assemble.set(Some(sr_raid1c_assemble));
    sd.sd_create.set(Some(sr_raid1c_create));
    sd.sd_free_resources.set(Some(sr_raid1c_free_resources));
    sd.sd_ioctl_handler.set(Some(sr_raid1c_ioctl));
    sd.sd_meta_opt_handler.set(Some(sr_raid1c_meta_opt_handler));
    sd.sd_scsi_rw.set(Some(sr_raid1c_rw));
    sd.sd_scsi_done.set(Some(sr_raid1c_done));
    sd.sd_scsi_wu_done.set(Some(sr_raid1_wu_done));
    sd.sd_set_chunk_state.set(Some(sr_raid1_set_chunk_state));
    sd.sd_set_vol_state.set(Some(sr_raid1_set_vol_state));
}

/// `sr_raid1c_create`: a new RAID 1C volume of `no_chunk` (two or more) chunks of
/// `coerced_size` blocks, with CRYPTO's metadata and keys.
pub fn sr_raid1c_create(
    sd: &'static SrDiscipline,
    bc: &mut BiocCreateraid,
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

    sr_raid1_init(sd)?;

    sr_crypto_meta_create(sd, &sd.mds().mdd_raid1c.sr1c_crypto, bc)
}

/// `sr_raid1c_add_offline_chunks`: grows `sv_chunks` from the `no_chunk` chunks found to the
/// volume's `ssd_chunk_no`, the missing ones offline place-holders (`NODEV`) linked after the
/// last chunk found.
pub fn sr_raid1c_add_offline_chunks(sd: &'static SrDiscipline, no_chunk: i32) -> Result<(), Errno> {
    let chunk_no = sd.sd_meta().ssdi().ssd_chunk_no.get() as usize;
    let no_chunk = usize::try_from(no_chunk).map_err(|_| Errno::EINVAL)?;
    if no_chunk == 0 || no_chunk > chunk_no {
        return Err(Errno::EINVAL);
    }

    let mut chunks: Vec<Option<&'static SrChunk>> = Vec::new();
    chunks
        .try_reserve_exact(no_chunk)
        .map_err(|_| Errno::ENOMEM)?;
    chunks.extend((0..no_chunk).map(|c| sd.sd_vol.sv_chunk_opt(c)));

    // free(sd->sd_vol.sv_chunks); sd->sd_vol.sv_chunks = chunks (mallocarray'd)
    sd.sd_vol.sv_chunks_alloc(chunk_no, M_WAITOK)?;
    for (c, ch) in chunks.into_iter().enumerate() {
        sd.sd_vol.set_sv_chunk(c, ch);
    }

    for c in no_chunk..chunk_no {
        let ch_prev = sd.sd_vol.sv_chunk(c - 1);
        let Some(p) = sr_malloc::<SrChunk>(M_WAITOK) else {
            return Err(Errno::ENOMEM);
        };
        // SAFETY: a zeroed chunk (`SrZeroed`); the volume owns it like the chunks found
        // (`sr_chunks_unwind` frees it).
        let ch_entry: &'static SrChunk = unsafe { p.as_ref() };
        ch_entry.src_meta.scm_status.set(BIOC_SDOFFLINE as u32);
        ch_entry.src_dev_mm.set(NODEV);
        // SAFETY: `ch_prev` is on the volume's `sv_chunk_list` (a chunk found, or the
        // place-holder linked just before); `ch_entry` is new and on no list.
        unsafe { SlistHead::<SrChunkLink>::insert_after(ch_prev, ch_entry) };
        sd.sd_vol.set_sv_chunk(c, Some(ch_entry));
    }

    Ok(())
}

/// `sr_raid1c_assemble`: brings up an existing volume, with place-holders for the chunks
/// that are missing, once its mask key is known.
pub fn sr_raid1c_assemble(
    sd: &'static SrDiscipline,
    bc: &mut BiocCreateraid,
    no_chunk: i32,
    data: Option<&[u8]>,
) -> Result<(), Errno> {
    // Create NODEV place-holders for missing chunks.
    if no_chunk < sd.sd_meta().ssdi().ssd_chunk_no.get() as i32 {
        sr_raid1c_add_offline_chunks(sd, no_chunk)?;
    }

    sr_raid1_assemble(sd, bc, no_chunk, None)?;

    sr_crypto_set_key(sd, &sd.mds().mdd_raid1c.sr1c_crypto, bc, no_chunk, data)
}

/// `sr_raid1c_ioctl`.
pub fn sr_raid1c_ioctl(sd: &'static SrDiscipline, bd: &mut BiocDiscipline) -> Result<(), Errno> {
    sr_crypto_ioctl_internal(sd, &sd.mds().mdd_raid1c.sr1c_crypto, bd)
}

/// `sr_raid1c_alloc_resources`.
pub fn sr_raid1c_alloc_resources(sd: &'static SrDiscipline) -> Result<(), Errno> {
    sr_crypto_alloc_resources_internal(sd, &sd.mds().mdd_raid1c.sr1c_crypto)
}

/// `sr_raid1c_free_resources`.
pub fn sr_raid1c_free_resources(sd: &'static SrDiscipline) {
    sr_crypto_free_resources_internal(sd, &sd.mds().mdd_raid1c.sr1c_crypto);
}

/// The chunk a read goes to: the next of the interleave (`sr1_counter`, which wraps) that is
/// online or scrubbing, trying past offline, rebuilding and hotspare chunks at most
/// `no_chunk` times; `None` when the volume is offline.
fn raid1c_read_chunk(
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
                // FALLTHROUGH: volume offline
                return None;
            }
            // volume offline
            _ => return None,
        }
    }
}

/// What a write does to a chunk in state `status` (`BIOC_SD*`): writes go on all working
/// disks; a rebuild's writes only on the chunks not online (the one being rebuilt).
fn raid1c_write_action(status: i32, rebuild: bool) -> Raid1cWrite {
    match status {
        BIOC_SDONLINE if rebuild => Raid1cWrite::Skip,
        BIOC_SDONLINE | BIOC_SDSCRUB | BIOC_SDREBUILD => Raid1cWrite::Write,
        // BIOC_SDHOTSPARE should never happen
        BIOC_SDHOTSPARE | BIOC_SDOFFLINE => Raid1cWrite::Skip,
        _ => Raid1cWrite::Bad,
    }
}

/// `sr_raid1c_dev_rw`: a read is one ccb on the next readable chunk, a write one ccb on
/// every working chunk, all writing `crwu`'s encrypted buffer (a rebuild writes the data it
/// read, as it is); the work unit is then scheduled.
pub fn sr_raid1c_dev_rw(
    wu: &'static SrWorkunit,
    crwu: Option<&'static SrCryptoWuReq>,
) -> Result<(), Errno> {
    let sd = wu.dis();
    let xs = wu.xs();
    let mdd_raid1c = &sd.mds().mdd_raid1c;

    let blkno = wu.swu_blk_start.get();
    let no_chunk = sd.sd_meta().ssdi().ssd_chunk_no.get();
    let read = xs.flags.get() & SCSI_DATA_IN != 0;
    let rebuild = wu.swu_flags.get() & SR_WUF_REBUILD != 0;

    let ios = if read { 1 } else { no_chunk };

    for i in 0..ios {
        let chunk = if read {
            let Some(chunk) =
                raid1c_read_chunk(&mdd_raid1c.sr1c_raid1.sr1_counter, no_chunk, |c| {
                    sd.sd_vol.sv_chunk(c).src_meta.scm_status.get() as i32
                })
            else {
                // volume offline
                printf(format_args!(
                    "{}: is offline, cannot read\n",
                    DEVNAME(sd.sd_sc())
                ));
                return Err(Errno::EINVAL);
            };
            chunk
        } else {
            // writes go on all working disks
            let chunk = i as usize;
            let scp = sd.sd_vol.sv_chunk(chunk);
            match raid1c_write_action(scp.src_meta.scm_status.get() as i32, rebuild) {
                Raid1cWrite::Write => chunk,
                Raid1cWrite::Skip => continue,
                Raid1cWrite::Bad => return Err(Errno::EINVAL),
            }
        };

        // SAFETY: `xs.data()` is valid and reserved for the transfer until it completes
        // (`ScsiXfer::set_data`), after all of its ccbs; every ccb reads or writes the whole
        // of it, as the C does (a write only reads its buffer; a read has one ccb).
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
            return Err(Errno::EINVAL);
        };
        if !read && !rebuild {
            ccb_write_crypted(ccb, crwu);
        }
        sr_wu_enqueue_ccb(wu, ccb);
    }

    sr_schedule_wu(wu);

    Ok(())
}

/// `sr_raid1c_meta_opt_handler`.
pub fn sr_raid1c_meta_opt_handler(
    sd: &'static SrDiscipline,
    omi: &'static SrMetaOptItem,
) -> Result<(), Errno> {
    sr_crypto_meta_opt_handler_internal(sd, &sd.mds().mdd_raid1c.sr1c_crypto, omi)
}

/// `sr_raid1c_rw`: a write (other than a rebuild's) is encrypted into the work unit's buffer
/// first; see [`sr_raid1c_dev_rw`].
pub fn sr_raid1c_rw(wu: &'static SrWorkunit) -> Result<(), Errno> {
    // DNPRINTF(SR_D_DIS, "%s: sr_raid1c_rw wu %p\n")

    sr_validate_io(wu, "sr_raid1c_rw")?;

    if wu.xs().flags.get() & SCSI_DATA_OUT != 0 && wu.swu_flags.get() & SR_WUF_REBUILD == 0 {
        let mdd_raid1c = &wu.dis().mds().mdd_raid1c;
        let (crwu, rv) = sr_crypto_prepare(wu, &mdd_raid1c.sr1c_crypto, true, crypto_invoke);

        // DNPRINTF(SR_D_INTR, "%s: sr_raid1c_rw: wu %p xs: %p\n")

        if rv.is_err() {
            // fail io (once: see the deviations)
            return Err(Errno::EIO);
        }

        sr_raid1c_dev_rw(wu, Some(crwu))
    } else {
        sr_raid1c_dev_rw(wu, None)
    }
}

/// `sr_raid1c_done`.
pub fn sr_raid1c_done(wu: &'static SrWorkunit) {
    sr_crypto_done_internal(wu, &wu.dis().mds().mdd_raid1c.sr1c_crypto);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Tests of RAID 1C: the read interleave and the write rule over every chunk state, the
    // place-holders for missing chunks, and the hooks the discipline installs.

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
}
/* </TESTS> */
