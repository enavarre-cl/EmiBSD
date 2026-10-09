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
//! `softraid_raid6.c`: the RAID 6 discipline of softraid(4). A volume of `n` chunks stores
//! its data in strips of `ssd_strip_size` bytes (`MAXPHYS`) dealt over `n - 2` chunks per
//! row, plus two parity strips per row on rotating chunks: P, the XOR of the row's data
//! strips, and Q, the sum in GF(2^8) (polynomial 0x11D) of each data strip `D` of chunk `i`
//! times `g^i` (`gf_pow[i]`, `g = 2`). Any two missing chunks of a row can be rebuilt on
//! read; a write reads the old data, P and Q in a second work unit that runs before it, and
//! updates both parities.
//!
//! Upstream: sys/dev/softraid_raid6.c @ 3ce1f3f79392
//!
//! The arithmetic of a strip (its data, P and Q chunks and its block) is [`Raid6Strip`]; the
//! ccbs of a strip are decided by `sr_raid6_strip`, the body of `sr_raid6_rw`'s loop, which
//! reaches the chunks through the small [`Raid6Io`] trait: the kernel's implementation is
//! `sr_raid6_addio`'s ccbs, the host tests' a set of in-memory chunks, so that the tests run
//! the same parity and recovery code on real bytes.
//!
//! ## Deviations
//! - Hooks return `Result<(), Errno>`; the C's `return (1)` and `sr_raid6_addio`'s `-1` are
//!   `Err(Errno::EIO)`; `sr_raid6_create` and `sr_raid6_init` keep their `EINVAL`.
//! - The GF(256) tables `gf_pow` and `gf_log` are built at compile time by the `const fn`
//!   `gf_init` (the C fills them, the same way, in every `sr_raid6_discipline_init`), so
//!   they are read-only. `gf_map`, the per-factor multiplication tables, are still allocated
//!   on first use by `gf_premul` (`M_NOWAIT`, which may fail); a table is installed with a
//!   compare-and-swap (the loser of a race frees its copy) and never freed, as in the C.
//! - `gf_inv(0)`, `gf_mul(0, 0)` and `gf_premul(0)` index outside the tables (undefined
//!   behaviour in C) and panic here; no caller passes them (the factors are powers of `g`,
//!   or products and sums of distinct powers).
//! - The loop body of `sr_raid6_rw` is `sr_raid6_strip`, generic over [`Raid6Io`] (the chunk
//!   status, `sr_raid6_addio`, `sr_block_get`/`put`, `memset`, `sr_raid6_xorp`/`xorq`).
//!   Buffers are [`Raid6Buf`] (a pointer and its length, made by an `unsafe` constructor
//!   that states the C's validity contract) instead of `void *`.
//! - Leaks the C marks with `XXX` are closed: a write strip puts its P and Q blocks back
//!   until a ccb owns them, `sr_raid6_addio` puts its ccb and its own block back when
//!   `gf_premul` or the opaque's `malloc` fails, and `sr_raid6_intr` frees the
//!   `struct sr_raid6_opaque` of a failed read too (the C frees it only on success). The
//!   ccbs already queued then never run (their work unit is put back unstarted), so nothing
//!   reads a freed block.
//! - `sr_raid6_rw` does not queue a write work unit on `sd_wu_defq` a second time when it is
//!   already there (`sr_raid_recreate_wu` re-running a deferred write): the C's
//!   `TAILQ_INSERT_TAIL` would corrupt the queue, which Rust's queue contract forbids. A
//!   write that comes without a read work unit (no `SCSI_DATA_OUT`, where the C would
//!   dereference NULL) fails.
//! - `sr_raid6_xorp`/`xorq` work on byte slices; like the C's 32-bit loops they process
//!   `len & !3` bytes (Q byte by byte through `gf_map`, as the C's four byte lanes do).
//! - `DNPRINTF` calls are comments (`SR_DEBUG` is not configured).

use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::dev::biovar::{
    BIOC_SDHOTSPARE, BIOC_SDOFFLINE, BIOC_SDONLINE, BIOC_SDREBUILD, BIOC_SDSCRUB, BIOC_SVBUILDING,
    BIOC_SVDEGRADED, BIOC_SVOFFLINE, BIOC_SVONLINE, BIOC_SVREBUILD, BIOC_SVSCRUB, BiocCreateraid,
};
use crate::dev::softraid::{
    sr_block_get, sr_block_put, sr_ccb_done, sr_ccb_put, sr_ccb_rw, sr_error, sr_schedule_wu,
    sr_scsi_wu_get, sr_scsi_wu_put, sr_validate_io, sr_validate_stripsize, sr_wu_done,
    sr_wu_enqueue_ccb, sr_wu_release_ccbs,
};
use crate::dev::softraidvar::*;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_task::task_add;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::intr::{splbio, splx};
use crate::scsi::scsiconf::{
    SCSI_DATA_IN, SCSI_DATA_OUT, SCSI_NOSLEEP, XS_DRIVER_STUFFUP, XS_NOERROR,
};
use crate::sys::buf::Buf;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_ZERO};
use crate::sys::param::{DEV_BSHIFT, MAXPHYS};
use crate::sys::task::SYSTQ;
use crate::sys::types::Daddr;

/// `SR_NOFAIL`.
pub(crate) const SR_NOFAIL: i32 = 0x00;
/// `SR_FAILX`: the strip's own data chunk is missing.
pub(crate) const SR_FAILX: i32 = 1 << 0;
/// `SR_FAILY`: another data chunk of the row is missing.
pub(crate) const SR_FAILY: i32 = 1 << 1;
/// `SR_FAILP`: the row's P chunk is missing.
pub(crate) const SR_FAILP: i32 = 1 << 2;
/// `SR_FAILQ`: the row's Q chunk is missing.
pub(crate) const SR_FAILQ: i32 = 1 << 3;

/// `struct sr_raid6_opaque`: what a read ccb XORs its data into when it completes.
#[derive(Debug)]
struct SrRaid6Opaque {
    /// `gn`: the GF(256) factor of the data for `qbuf`.
    gn: i32,
    /// `pbuf`: the buffer the data is XORed into, or NULL.
    pbuf: *mut u8,
    /// `qbuf`: the buffer `gn` times the data is XORed into, or NULL.
    qbuf: *mut u8,
}

// SAFETY: an integer and two raw pointers (NULL); no `Drop`.
unsafe impl SrZeroed for SrRaid6Opaque {}

/// The GF(256) tables `gf_init` builds.
pub(crate) struct GfTables {
    /// `gf_pow`: `g^i` for `i` in `0..=510` (a 2N table, to avoid using % in multiply); zero
    /// from 511 on, which `gf_log[0] == 512` reaches for a zero factor.
    pub(crate) gf_pow: [u8; 768],
    /// `gf_log`: `log_g(a)`, with `gf_log[1] == 255` and `gf_log[0] == 512`.
    pub(crate) gf_log: [i32; 256],
}

/// One strip of a RAID 6 transfer: where `sr_raid6_rw` finds it on the chunks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Raid6Strip {
    /// `strip_no`: the strip's number in the volume's data.
    pub(crate) strip_no: i64,
    /// `chunk`: the chunk that holds it.
    pub(crate) chunk: i64,
    /// `pchunk`: the chunk that holds its row's P (xor) parity.
    pub(crate) pchunk: i64,
    /// `qchunk`: the chunk that holds its row's Q (GF(256)) parity.
    pub(crate) qchunk: i64,
    /// `lba`: its block on all three (relative to the data area).
    pub(crate) lba: Daddr,
    /// `length`: the bytes of the transfer in it.
    pub(crate) length: i64,
}

impl Raid6Strip {
    /// The strip at byte `lbaoffs` of the volume, `datalen` bytes of the transfer left;
    /// `no_chunk` is the number of data chunks of a row (`ssd_chunk_no - 2`).
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

        // map disk offset to parity/data drive
        let mut chunk = strip_no % no_chunk;

        let qchunk = (no_chunk + 1) - ((strip_no / no_chunk) % (no_chunk + 2));
        let pchunk = if qchunk == 0 {
            no_chunk + 1
        } else {
            qchunk - 1
        };
        if chunk >= pchunk {
            chunk += 1;
        }
        if chunk >= qchunk {
            chunk += 1;
        }

        Raid6Strip {
            strip_no,
            chunk,
            pchunk,
            qchunk,
            lba: offset >> DEV_BSHIFT,
            length,
        }
    }
}

/// A buffer of a strip's I/O (the C's `void *`): the transfer's data at the strip, or an
/// `sr_block_get` block.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Raid6Buf {
    /// The first byte.
    ptr: NonNull<u8>,
    /// The bytes it may be used for.
    len: usize,
}

impl Raid6Buf {
    /// The buffer of `len` bytes at `ptr`; `None` for NULL or a negative length.
    ///
    /// # Safety
    ///
    /// `ptr` is valid for reads and writes of `len` bytes, which nothing but this discipline's
    /// I/O touches, until the I/O that uses the buffer has completed and (for a block) the
    /// block has been put back.
    unsafe fn new(ptr: *mut u8, len: i64) -> Option<Self> {
        Some(Raid6Buf {
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
                "sr_raid6: {} bytes past a buffer of {}",
                len, self.len
            )),
        }
    }
}

/// The kernel's [`Raid6Io`]: ccbs on the chunks of a discipline.
struct Raid6Ccbs {
    /// The discipline.
    sd: &'static SrDiscipline,
}

/// `gf_pow`, `gf_log`: built by `gf_init` at compile time.
static GF: GfTables = gf_init();

/// `gf_map`: `gf_map[gn][x] == gn * x`, a 256-byte table per factor `gn`, allocated by
/// `gf_premul` on first use and never freed.
static GF_MAP: [AtomicPtr<u8>; 256] = [const { AtomicPtr::new(ptr::null_mut()) }; 256];

/// The chunk I/O that `sr_raid6_strip` builds a strip's ccbs from.
pub(crate) trait Raid6Io {
    /// A work unit (`struct sr_workunit *`).
    type Wu: Copy;
    /// A buffer (`void *`).
    type Buf: Copy;

    /// `sd->sd_vol.sv_chunks[chunk]->src_meta.scm_status`.
    fn chunk_status(&self, chunk: i64) -> i32;
    /// `sr_raid6_addio`: queues on `wu` a read (`SCSI_DATA_IN`) or write of `len` bytes at
    /// `blkno` of `chunk` into or from `data` (`None`: a block of its own, freed when the ccb
    /// completes); when a read completes its data is XORed into `pbuf` and `gn` times its
    /// data into `qbuf`.
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
        pbuf: Option<Self::Buf>,
        qbuf: Option<Self::Buf>,
        gn: u8,
    ) -> Result<(), Errno>;
    /// `sr_block_get`.
    fn block_get(&mut self, len: i64) -> Option<Self::Buf>;
    /// `sr_block_put`.
    fn block_put(&mut self, buf: Self::Buf, len: i64);
    /// `memset(buf, 0, len)`.
    fn zero(&mut self, buf: Self::Buf, len: i64);
    /// `sr_raid6_xorp(p, d, len)`.
    fn xorp(&mut self, p: Self::Buf, d: Self::Buf, len: i64);
    /// `sr_raid6_xorq(q, d, len, gn)`.
    fn xorq(&mut self, q: Self::Buf, d: Self::Buf, len: i64, gn: u8);
}

/// `sr_raid6_discipline_init`: discipline initialisation.
pub fn sr_raid6_discipline_init(sd: &'static SrDiscipline) {
    // Initialize GF256 tables: `GF` is built at compile time by `gf_init`.

    // Fill out discipline members.
    sd.sd_type.set(SR_MD_RAID6);
    sd.sd_name.set(*b"RAID 6\0\0\0\0");
    sd.sd_capabilities
        .set(SR_CAP_SYSTEM_DISK | SR_CAP_AUTO_ASSEMBLE | SR_CAP_REDUNDANT);
    sd.sd_max_wu.set(SR_RAID6_NOWU);

    // Setup discipline specific function pointers.
    sd.sd_assemble.set(Some(sr_raid6_assemble));
    sd.sd_create.set(Some(sr_raid6_create));
    sd.sd_openings.set(Some(sr_raid6_openings));
    sd.sd_scsi_rw.set(Some(sr_raid6_rw));
    sd.sd_scsi_intr.set(Some(sr_raid6_intr));
    sd.sd_scsi_wu_done.set(Some(sr_raid6_wu_done));
    sd.sd_set_chunk_state.set(Some(sr_raid6_set_chunk_state));
    sd.sd_set_vol_state.set(Some(sr_raid6_set_vol_state));
}

/// `sr_raid6_create`: sets up the metadata of a new RAID 6 volume of `no_chunk` chunks of
/// `coerced_size` blocks.
pub fn sr_raid6_create(
    sd: &'static SrDiscipline,
    _bc: &mut BiocCreateraid,
    no_chunk: i32,
    coerced_size: i64,
) -> Result<(), Errno> {
    if no_chunk < 4 {
        sr_error(
            sd.sd_sc(),
            format_args!("{} requires four or more chunks", sd.name()),
        );
        return Err(Errno::EINVAL);
    }

    // XXX add variable strip size later even though MAXPHYS is really the clever value,
    // users like to tinker with that type of stuff.
    let ssdi = sd.sd_meta().ssdi();
    ssdi.ssd_strip_size.set(MAXPHYS as u32);
    ssdi.ssd_size.set(raid6_volume_size(
        coerced_size,
        ssdi.ssd_strip_size.get(),
        no_chunk,
    ));

    sr_raid6_init(sd)
}

/// The size in blocks of a RAID 6 volume of `no_chunk` chunks of `coerced_size` blocks: the
/// chunk size truncated to whole strips, times the `no_chunk - 2` data chunks of a row.
pub(crate) fn raid6_volume_size(coerced_size: i64, strip_size: u32, no_chunk: i32) -> i64 {
    let strip_blocks = u64::from(strip_size) >> DEV_BSHIFT;
    ((coerced_size as u64 & !(strip_blocks.wrapping_sub(1)))
        .wrapping_mul(i64::from(no_chunk - 2) as u64)) as i64
}

/// `sr_raid6_assemble`: brings up an existing RAID 6 volume.
pub fn sr_raid6_assemble(
    sd: &'static SrDiscipline,
    _bc: &mut BiocCreateraid,
    _no_chunk: i32,
    _data: Option<&[u8]>,
) -> Result<(), Errno> {
    sr_raid6_init(sd)
}

/// `sr_raid6_init`: initialises the runtime values (the strip shift and the ccb budget).
pub fn sr_raid6_init(sd: &'static SrDiscipline) -> Result<(), Errno> {
    let ssdi = sd.sd_meta().ssdi();

    // Initialise runtime values.
    let strip_bits = sr_validate_stripsize(ssdi.ssd_strip_size.get()).unwrap_or(-1);
    sd.mds().mdd_raid6.sr6_strip_bits.set(strip_bits);
    if strip_bits == -1 {
        sr_error(sd.sd_sc(), format_args!("invalid strip size"));
        return Err(Errno::EINVAL);
    }

    // only if stripsize <= MAXPHYS
    sd.sd_max_ccb_per_wu
        .set(6u32.max(2 * ssdi.ssd_chunk_no.get()));

    Ok(())
}

/// `sr_raid6_openings`: 2 wu's per IO.
pub fn sr_raid6_openings(sd: &'static SrDiscipline) -> i32 {
    (sd.sd_max_wu.get() >> 1) as i32
}

/// Whether a RAID 6 chunk may go from `old_state` to `new_state` (`BIOC_SD*`, different).
pub(crate) fn raid6_chunk_state_ok(old_state: i32, new_state: i32) -> bool {
    match old_state {
        BIOC_SDONLINE => matches!(new_state, BIOC_SDOFFLINE | BIOC_SDSCRUB),
        BIOC_SDOFFLINE => new_state == BIOC_SDREBUILD,
        BIOC_SDSCRUB | BIOC_SDREBUILD => matches!(new_state, BIOC_SDONLINE | BIOC_SDOFFLINE),
        _ => false,
    }
}

/// `sr_raid6_set_chunk_state`: chunk `c` goes to `new_state`; panics on a transition RAID 6
/// does not know.
pub fn sr_raid6_set_chunk_state(sd: &'static SrDiscipline, c: usize, new_state: i32) {
    let chunk = sd.sd_vol.sv_chunk(c);

    // XXX this is for RAID 0
    // DNPRINTF(SR_D_STATE, "%s: %s: %s: sr_raid_set_chunk_state %d -> %d")

    // ok to go to splbio since this only happens in error path
    let s = splbio();
    let old_state = chunk.src_meta.scm_status.get() as i32;

    // multiple IOs to the same chunk that fail will come through here
    if old_state != new_state {
        if !raid6_chunk_state_ok(old_state, new_state) {
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

/// The state of a RAID 6 volume of `nd` chunks that count `states[s]` chunks in each state
/// `s`; `None` when no state fits (the C panics).
pub(crate) fn raid6_vol_state(states: &[i64; SR_MAX_STATES], nd: i64) -> Option<i32> {
    let online = states[BIOC_SDONLINE as usize];
    if online == nd {
        Some(BIOC_SVONLINE)
    } else if online < nd - 2 {
        Some(BIOC_SVOFFLINE)
    } else if states[BIOC_SDSCRUB as usize] != 0 {
        Some(BIOC_SVSCRUB)
    } else if states[BIOC_SDREBUILD as usize] != 0 {
        Some(BIOC_SVREBUILD)
    } else if online < nd {
        Some(BIOC_SVDEGRADED)
    } else {
        None
    }
}

/// Whether a RAID 6 volume may go from `old_state` to `new_state` (`BIOC_SV*`).
pub(crate) fn raid6_vol_state_ok(old_state: i32, new_state: i32) -> bool {
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

/// `sr_raid6_set_vol_state`: the volume's state from its chunks'; panics on a transition
/// RAID 6 does not know.
pub fn sr_raid6_set_vol_state(sd: &'static SrDiscipline) {
    let mut states = [0i64; SR_MAX_STATES];
    let old_state = sd.sd_vol_status.get();

    // XXX this is for RAID 0

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

    let Some(new_state) = raid6_vol_state(&states, nd as i64) else {
        printf(format_args!("old_state = {}, ", old_state));
        for i in 0..nd {
            printf(format_args!(
                "{} = {}, ",
                i,
                sd.sd_vol.sv_chunk(i).src_meta.scm_status.get()
            ));
        }
        panic(format_args!("invalid new_state"));
    };

    // DNPRINTF(SR_D_STATE, "%s: %s: sr_raid_set_vol_state %d -> %d")

    if !raid6_vol_state_ok(old_state, new_state) {
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

//  modes:
//   readq: sr_raid6_addio(i, lba, length, NULL, SCSI_DATA_IN, 0, qbuf, NULL, 0);
//   readp: sr_raid6_addio(i, lba, length, NULL, SCSI_DATA_IN, 0, pbuf, NULL, 0);
//   readx: sr_raid6_addio(i, lba, length, NULL, SCSI_DATA_IN, 0, pbuf, qbuf, gf_pow[i]);

/// `sr_raid6_rw`: the `sd_scsi_rw` hook: splits a read or write into strips and queues the
/// ccbs of each ([`sr_raid6_strip`]); a write gets a second work unit for the reads of the
/// old data and parities, which runs first.
pub fn sr_raid6_rw(wu: &'static SrWorkunit) -> Result<(), Errno> {
    let sd = wu.dis();
    let xs = wu.xs();

    // blkno and scsi error will be handled by sr_validate_io
    let blkno = sr_validate_io(wu, "sr_raid6_rw")?;

    let mut wu_r: Option<&'static SrWorkunit> = None;
    if xs.flags.get() & SCSI_DATA_OUT != 0 {
        let Some(r) = sr_scsi_wu_get(sd, SCSI_NOSLEEP) else {
            printf(format_args!("{}: can't get wu_r", DEVNAME(sd.sd_sc())));
            return Err(Errno::EIO);
        };
        r.swu_state.set(SR_WU_INPROGRESS);
        r.swu_flags.set(r.swu_flags.get() | SR_WUF_DISCIPLINE);
        wu_r = Some(r);
    }

    if let Err(e) = sr_raid6_rw_strips(sd, wu, wu_r, blkno) {
        // wu is unwound by sr_wu_put
        if let Some(r) = wu_r {
            sr_scsi_wu_put(sd, r);
        }
        return Err(e);
    }

    let mut sched = wu;
    let s = splbio();
    if let Some(r) = wu_r {
        // collide write request with reads
        r.swu_blk_start.set(wu.swu_blk_start.get());
        r.swu_blk_end.set(wu.swu_blk_end.get());

        wu.swu_state.set(SR_WU_DEFERRED);
        r.swu_collider.set(Some(wu));
        if !sd.sd_wu_defq.iter().any(|w| ptr::eq(w, wu)) {
            // SAFETY: the work unit is on no other processing queue (being built, or
            // restarted off the pending queue), and not on this one (checked); work units
            // live until `sr_wu_free`; at `splbio`.
            unsafe { sd.sd_wu_defq.insert_tail(wu) };
        }

        sched = r;
    }
    splx(s);

    sr_schedule_wu(sched);

    Ok(())
}

/// The strip loop of `sr_raid6_rw`: queues the ccbs of every strip of the transfer of `wu`
/// at volume block `blkno`.
fn sr_raid6_rw_strips(
    sd: &'static SrDiscipline,
    wu: &'static SrWorkunit,
    wu_r: Option<&'static SrWorkunit>,
    blkno: Daddr,
) -> Result<(), Errno> {
    let xs = wu.xs();
    let ssdi = sd.sd_meta().ssdi();
    let mut io = Raid6Ccbs { sd };

    let strip_size = i64::from(ssdi.ssd_strip_size.get());
    let strip_bits = i64::from(sd.mds().mdd_raid6.sr6_strip_bits.get());
    let no_chunk = i64::from(ssdi.ssd_chunk_no.get()) - 2;
    let row_size = (no_chunk << strip_bits) >> DEV_BSHIFT;

    let data = xs.data();
    let mut datalen = i64::from(xs.datalen());
    let mut lbaoffs = blkno << DEV_BSHIFT;
    let mut done: usize = 0;

    wu.swu_blk_start.set(0);
    while datalen != 0 {
        let st = Raid6Strip::new(lbaoffs, datalen, strip_size, strip_bits, no_chunk);
        let row = st.strip_no / no_chunk;

        // XXX big hammer.. exclude I/O from entire stripe
        if wu.swu_blk_start.get() == 0 {
            wu.swu_blk_start.set(row * row_size);
        }
        wu.swu_blk_end.set(row * row_size + (row_size - 1));

        // SAFETY: `done + st.length <= xs->datalen`, and the transfer's data is valid for
        // `datalen` bytes until it completes (`ScsiXfer::set_data`), which is after every ccb
        // of this work unit has; the discipline is the transfer's adapter meanwhile.
        let Some(buf) = (unsafe { Raid6Buf::new(data.wrapping_add(done), st.length) }) else {
            return Err(Errno::EIO);
        };

        sr_raid6_strip(&mut io, wu, wu_r, xs.flags.get(), &st, no_chunk, buf)?;

        // advance to next block
        lbaoffs += st.length;
        datalen -= st.length;
        done += st.length as usize;
    }

    Ok(())
}

/// The body of `sr_raid6_rw`'s loop: queues the ccbs of strip `st` (`no_chunk` data chunks
/// per row) of a read (`SCSI_DATA_IN` in `xsflags`) into or a write from `data`: a plain
/// read, a regeneration of a missing data chunk from the others and P or Q, or the write of
/// the data with its new P and Q, which `wu_r` reads the old values for.
pub(crate) fn sr_raid6_strip<I: Raid6Io>(
    io: &mut I,
    wu: I::Wu,
    wu_r: Option<I::Wu>,
    xsflags: i32,
    st: &Raid6Strip,
    no_chunk: i64,
    data: I::Buf,
) -> Result<(), Errno> {
    let Raid6Strip {
        chunk,
        pchunk,
        qchunk,
        lba,
        length,
        ..
    } = *st;

    let mut fail = SR_NOFAIL;
    let mut fchunk = -1;

    // Get disk-fail flags
    for i in 0..no_chunk + 2 {
        if matches!(
            io.chunk_status(i),
            BIOC_SDOFFLINE | BIOC_SDREBUILD | BIOC_SDHOTSPARE
        ) {
            if i == qchunk {
                fail |= SR_FAILQ;
            } else if i == pchunk {
                fail |= SR_FAILP;
            } else if i == chunk {
                fail |= SR_FAILX;
            } else {
                // dual data-disk failure
                fail |= SR_FAILY;
                fchunk = i;
            }
        }
    }

    if xsflags & SCSI_DATA_IN != 0 {
        if fail & SR_FAILX == 0 {
            // drive is good. issue single read request
            io.addio(
                wu,
                chunk,
                lba,
                length,
                Some(data),
                xsflags,
                0,
                None,
                None,
                0,
            )?;
        } else if fail & SR_FAILP != 0 {
            // Dx, P failed
            printf(format_args!(
                "Disk {:x} offline, regenerating Dx+P\n",
                chunk
            ));

            let gxinv = gf_inv(gf_pow(chunk));

            // Calculate: Dx = (Q^Dz*gz)*inv(gx)
            io.zero(data, length);
            io.addio(
                wu,
                qchunk,
                lba,
                length,
                None,
                SCSI_DATA_IN,
                0,
                None,
                Some(data),
                gxinv,
            )?;

            // Read Dz * gz * inv(gx)
            for i in 0..no_chunk + 2 {
                if i == qchunk || i == pchunk || i == chunk {
                    continue;
                }

                io.addio(
                    wu,
                    i,
                    lba,
                    length,
                    None,
                    SCSI_DATA_IN,
                    0,
                    None,
                    Some(data),
                    gf_mul(gf_pow(i), gxinv),
                )?;
            }

            // data will contain correct value on completion
        } else if fail & SR_FAILY != 0 {
            // Dx, Dy failed
            printf(format_args!(
                "Disk {:x} & {:x} offline, regenerating Dx+Dy\n",
                chunk, fchunk
            ));

            let gxinv = gf_inv(gf_pow(chunk) ^ gf_pow(fchunk));
            let pxinv = gf_mul(gf_pow(fchunk), gxinv);

            // read Q * inv(gx + gy)
            io.zero(data, length);
            io.addio(
                wu,
                qchunk,
                lba,
                length,
                None,
                SCSI_DATA_IN,
                0,
                None,
                Some(data),
                gxinv,
            )?;

            // read P * gy * inv(gx + gy)
            io.addio(
                wu,
                pchunk,
                lba,
                length,
                None,
                SCSI_DATA_IN,
                0,
                None,
                Some(data),
                pxinv,
            )?;

            // Calculate: Dx*gx^Dy*gy = Q^(Dz*gz) ; Dx^Dy = P^Dz
            //   Q:  sr_raid6_xorp(qbuf, --, length);
            //   P:  sr_raid6_xorp(pbuf, --, length);
            //   Dz: sr_raid6_xorp(pbuf, --, length);
            //       sr_raid6_xorq(qbuf, --, length, gf_pow[i]);
            for i in 0..no_chunk + 2 {
                if i == qchunk || i == pchunk || i == chunk || i == fchunk {
                    continue;
                }

                // read Dz * (gz + gy) * inv(gx + gy)
                io.addio(
                    wu,
                    i,
                    lba,
                    length,
                    None,
                    SCSI_DATA_IN,
                    0,
                    None,
                    Some(data),
                    pxinv ^ gf_mul(gf_pow(i), gxinv),
                )?;
            }
        } else {
            // Two cases: single disk (Dx) or (Dx+Q)
            //   Dx = Dz ^ P (same as RAID5)
            printf(format_args!(
                "Disk {:x} offline, regenerating Dx{}\n",
                chunk,
                if fail & SR_FAILQ != 0 {
                    "+Q"
                } else {
                    " single"
                }
            ));

            // Calculate: Dx = P^Dz
            //   P:  sr_raid6_xorp(data, ---, length);
            //   Dz: sr_raid6_xorp(data, ---, length);
            io.zero(data, length);
            for i in 0..no_chunk + 2 {
                if i != chunk && i != qchunk {
                    // Read Dz
                    io.addio(
                        wu,
                        i,
                        lba,
                        length,
                        None,
                        SCSI_DATA_IN,
                        0,
                        Some(data),
                        None,
                        0,
                    )?;
                }
            }

            // data will contain correct value on completion
        }
    } else {
        // XXX handle writes to failed/offline disk?
        if fail & (SR_FAILX | SR_FAILQ | SR_FAILP) != 0 {
            return Err(Errno::EIO);
        }
        let Some(wu_r) = wu_r else {
            return Err(Errno::EIO);
        };

        // initialize pbuf with contents of new data to be written. This will be XORed with
        // old data and old parity in the intr routine. The result in pbuf is the new parity
        // data.
        let Some(qbuf) = io.block_get(length) else {
            return Err(Errno::EIO);
        };
        let Some(pbuf) = io.block_get(length) else {
            io.block_put(qbuf, length);
            return Err(Errno::EIO);
        };

        // The blocks are ours to put back until their write ccb takes them (or
        // sr_raid6_addio puts one back when it cannot queue that ccb).
        let mut own_p = true;
        let mut own_q = true;
        let res: Result<(), Errno> = 'bad: {
            let gn = gf_pow(chunk);

            // Calculate P = Dn; Q = gn * Dn
            if gf_premul(gn).is_err() {
                break 'bad Err(Errno::EIO);
            }
            io.xorp(pbuf, data, length);
            io.xorq(qbuf, data, length, gn);

            // Read old data: P ^= Dn' ; Q ^= (gn * Dn')
            if let Err(e) = io.addio(
                wu_r,
                chunk,
                lba,
                length,
                None,
                SCSI_DATA_IN,
                0,
                Some(pbuf),
                Some(qbuf),
                gn,
            ) {
                break 'bad Err(e);
            }

            // Read old xor-parity: P ^= P'
            if let Err(e) = io.addio(
                wu_r,
                pchunk,
                lba,
                length,
                None,
                SCSI_DATA_IN,
                0,
                Some(pbuf),
                None,
                0,
            ) {
                break 'bad Err(e);
            }

            // Read old q-parity: Q ^= Q'
            if let Err(e) = io.addio(
                wu_r,
                qchunk,
                lba,
                length,
                None,
                SCSI_DATA_IN,
                0,
                Some(qbuf),
                None,
                0,
            ) {
                break 'bad Err(e);
            }

            // write new data
            if let Err(e) = io.addio(
                wu,
                chunk,
                lba,
                length,
                Some(data),
                xsflags,
                0,
                None,
                None,
                0,
            ) {
                break 'bad Err(e);
            }

            // write new xor-parity
            own_p = false;
            if let Err(e) = io.addio(
                wu,
                pchunk,
                lba,
                length,
                Some(pbuf),
                xsflags,
                SR_CCBF_FREEBUF,
                None,
                None,
                0,
            ) {
                break 'bad Err(e);
            }

            // write new q-parity
            own_q = false;
            io.addio(
                wu,
                qchunk,
                lba,
                length,
                Some(qbuf),
                xsflags,
                SR_CCBF_FREEBUF,
                None,
                None,
                0,
            )
        };
        if res.is_err() {
            if own_p {
                io.block_put(pbuf, length);
            }
            if own_q {
                io.block_put(qbuf, length);
            }
        }
        res?;
    }

    Ok(())
}

/// `sr_raid6_intr`: the `sd_scsi_intr` hook: a ccb is done; XORs a read into its P and Q
/// buffers (`struct sr_raid6_opaque`) and frees its own block.
pub fn sr_raid6_intr(bp: &'static Buf) {
    // SAFETY: `sr_raid6_intr` is only installed as `sd_scsi_intr`, which `sr_ccb_rw` makes
    // the `b_iodone` of a ccb's own buffer.
    let ccb = unsafe { sr_ccb_from_buf(bp) };
    let wu = ccb.wu();
    let sd = wu.dis();

    // DNPRINTF(SR_D_INTR, "%s: sr_raid6_intr bp %p xs %p")

    let s = splbio();
    sr_ccb_done(ccb);

    let len = usize::try_from(ccb.ccb_buf.b_bcount.get()).unwrap_or(0);
    let b_data = ccb.ccb_buf.b_data.get();

    // XOR data to result.
    if let Some(pq) = NonNull::new(ccb.ccb_opaque.get().cast::<SrRaid6Opaque>()) {
        if ccb.ccb_state.get() == SR_CCB_OK && !b_data.is_null() {
            // SAFETY: `sr_raid6_addio` sets `ccb_opaque` to an opaque it allocated and filled,
            // freed only here.
            let pq = unsafe { pq.as_ref() };
            // SAFETY: `sr_raid6_addio` sets an opaque only for a read into a block of its
            // own (`data` NULL), so `b_data` is that block of `b_bcount` bytes; the opaque's
            // buffers are other buffers (the transfer's data or P and Q blocks) of the same
            // length, valid until the work unit completes, which waits for this ccb; ccbs
            // complete one at a time at `splbio`.
            let d = unsafe { slice::from_raw_parts(b_data.cast_const(), len) };
            if !pq.pbuf.is_null() {
                // Calculate xor-parity
                // SAFETY: as above; no other reference to `pbuf` lives meanwhile.
                let p = unsafe { slice::from_raw_parts_mut(pq.pbuf, len) };
                sr_raid6_xorp(p, d, len);
            }
            if !pq.qbuf.is_null() {
                // Calculate q-parity
                // SAFETY: as above; `qbuf` is not `pbuf` (`sr_raid6_addio`'s callers).
                let q = unsafe { slice::from_raw_parts_mut(pq.qbuf, len) };
                sr_raid6_xorq(q, d, len, pq.gn as u8);
            }
        }
        sr_free(pq, size_of::<SrRaid6Opaque>());
        ccb.ccb_opaque.set(ptr::null_mut());
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

/// `sr_raid6_wu_done`: the `sd_scsi_wu_done` hook: a transfer succeeds when any of its ccbs
/// did; a failed read is restarted once more.
pub fn sr_raid6_wu_done(wu: &'static SrWorkunit) -> i32 {
    let sd = wu.dis();

    // XXX - we have no way of propagating errors...
    if wu.swu_flags.get() & SR_WUF_DISCIPLINE != 0 {
        return SR_WU_OK;
    }

    let xs = wu.xs();

    // XXX - This is insufficient for RAID 6.
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

/// `sr_raid6_addio`: queues on `wu` a ccb for `len` bytes at `blkno` of `chunk` (see
/// [`Raid6Io::addio`]).
#[allow(clippy::too_many_arguments)] // the C's signature
fn sr_raid6_addio(
    wu: &'static SrWorkunit,
    chunk: i64,
    blkno: Daddr,
    len: i64,
    data: Option<Raid6Buf>,
    xsflags: i32,
    mut ccbflags: i32,
    pbuf: Option<Raid6Buf>,
    qbuf: Option<Raid6Buf>,
    gn: u8,
) -> Result<(), Errno> {
    let sd = wu.dis();

    // DNPRINTF(SR_D_DIS, "sr_raid6_addio: %s %d.%lld %ld %p:%p")

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
            match unsafe { Raid6Buf::new(block.as_ptr(), len) } {
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
    // SAFETY: `data` is valid for `n == len` bytes until the ccb completes (`Raid6Buf`'s
    // contract: the transfer's data, or a block the ccb frees when done).
    let ccb = unsafe { sr_ccb_rw(sd, chunk, blkno, n as i64, data.as_ptr(), xsflags, ccbflags) };
    let Some(ccb) = ccb else {
        if ccbflags & SR_CCBF_FREEBUF != 0 {
            sr_block_put(sd, data.ptr, len);
        }
        return Err(Errno::EIO);
    };

    if pbuf.is_some() || qbuf.is_some() {
        // The C can leak data and ccb on failure here; both go back.
        let unwind = || {
            sr_ccb_put(ccb);
            if ccbflags & SR_CCBF_FREEBUF != 0 {
                sr_block_put(sd, data.ptr, len);
            }
        };
        if qbuf.is_some() && gf_premul(gn).is_err() {
            unwind();
            return Err(Errno::EIO);
        }

        // XXX - should be preallocated?
        let Some(pqbuf) = sr_malloc::<SrRaid6Opaque>(M_NOWAIT) else {
            unwind();
            return Err(Errno::EIO);
        };
        // SAFETY: a fresh, suitably aligned allocation of one `SrRaid6Opaque`, freed by
        // `sr_raid6_intr`.
        unsafe {
            pqbuf.as_ptr().write(SrRaid6Opaque {
                gn: i32::from(gn),
                pbuf: pbuf.map_or(ptr::null_mut(), Raid6Buf::as_ptr),
                qbuf: qbuf.map_or(ptr::null_mut(), Raid6Buf::as_ptr),
            })
        };
        ccb.ccb_opaque.set(pqbuf.as_ptr().cast());
    }
    sr_wu_enqueue_ccb(wu, ccb);

    Ok(())
}

impl Raid6Io for Raid6Ccbs {
    type Wu = &'static SrWorkunit;
    type Buf = Raid6Buf;

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
        data: Option<Raid6Buf>,
        xsflags: i32,
        ccbflags: i32,
        pbuf: Option<Raid6Buf>,
        qbuf: Option<Raid6Buf>,
        gn: u8,
    ) -> Result<(), Errno> {
        sr_raid6_addio(
            wu, chunk, blkno, len, data, xsflags, ccbflags, pbuf, qbuf, gn,
        )
    }

    fn block_get(&mut self, len: i64) -> Option<Raid6Buf> {
        let block = sr_block_get(self.sd, len)?;
        // SAFETY: a fresh block of `len` bytes; whoever takes it puts it back.
        let buf = unsafe { Raid6Buf::new(block.as_ptr(), len) };
        if buf.is_none() {
            sr_block_put(self.sd, block, len);
        }
        buf
    }

    fn block_put(&mut self, buf: Raid6Buf, len: i64) {
        sr_block_put(self.sd, buf.ptr, len);
    }

    fn zero(&mut self, buf: Raid6Buf, len: i64) {
        let n = buf.span(len);
        // SAFETY: `buf` is valid for writes of its `len >= n` bytes (`Raid6Buf`).
        unsafe { ptr::write_bytes(buf.as_ptr(), 0, n) };
    }

    fn xorp(&mut self, p: Raid6Buf, d: Raid6Buf, len: i64) {
        let n = p.span(len).min(d.span(len));
        if ptr::eq(p.as_ptr(), d.as_ptr()) {
            panic(format_args!("sr_raid6_xorp: buffer xored into itself"));
        }
        // SAFETY: both are valid for `n` bytes (`Raid6Buf`) and are different buffers
        // (checked: the callers' P block and the transfer's data).
        let (p, d) = unsafe {
            (
                slice::from_raw_parts_mut(p.as_ptr(), n),
                slice::from_raw_parts(d.as_ptr().cast_const(), n),
            )
        };
        sr_raid6_xorp(p, d, n);
    }

    fn xorq(&mut self, q: Raid6Buf, d: Raid6Buf, len: i64, gn: u8) {
        let n = q.span(len).min(d.span(len));
        if ptr::eq(q.as_ptr(), d.as_ptr()) {
            panic(format_args!("sr_raid6_xorq: buffer xored into itself"));
        }
        // SAFETY: as in `xorp`.
        let (q, d) = unsafe {
            (
                slice::from_raw_parts_mut(q.as_ptr(), n),
                slice::from_raw_parts(d.as_ptr().cast_const(), n),
            )
        };
        sr_raid6_xorq(q, d, n, gn);
    }
}

/// `sr_raid6_xorp`: perform RAID6 parity calculation: `p ^= d` over the first `len` bytes,
/// in the C's 32-bit words (`len & !3` bytes). P=xor parity, D=data.
pub(crate) fn sr_raid6_xorp(p: &mut [u8], d: &[u8], len: usize) {
    let n = len & !3;
    for (x, y) in p[..n].iter_mut().zip(&d[..n]) {
        *x ^= *y;
    }
}

/// `sr_raid6_xorq`: `q ^= gn * d` in GF(256) over the first `len` bytes (`len & !3`), through
/// `gf_map[gn]`, which `gf_premul(gn)` must have made. Q=GF256 parity, gn=disk factor.
pub(crate) fn sr_raid6_xorq(q: &mut [u8], d: &[u8], len: usize, gn: u8) {
    let gn_map = gf_map(gn);
    let n = len & !3;
    for (x, y) in q[..n].iter_mut().zip(&d[..n]) {
        *x ^= gn_map[usize::from(*y)];
    }
}

/// `gf_init`: create GF256 log/pow tables: polynomial = 0x11D.
pub(crate) const fn gf_init() -> GfTables {
    let mut t = GfTables {
        gf_pow: [0; 768],
        gf_log: [0; 256],
    };
    let mut p: u8 = 1;

    // use 2N pow table to avoid using % in multiply
    let mut i = 0;
    while i < 256 {
        t.gf_log[p as usize] = i as i32;
        t.gf_pow[i] = p;
        t.gf_pow[i + 255] = p;
        p = (p << 1) ^ if p & 0x80 != 0 { 0x1D } else { 0x00 };
        i += 1;
    }
    t.gf_log[0] = 512;
    t
}

/// `gf_pow[i]`: `g^i`.
fn gf_pow(i: i64) -> u8 {
    GF.gf_pow[i as usize]
}

/// `gf_inv`: the GF(256) inverse of `a` (panics for 0, which has none).
pub(crate) fn gf_inv(a: u8) -> u8 {
    GF.gf_pow[(255 - GF.gf_log[usize::from(a)]) as usize]
}

/// `gf_mul`: `a * b` in GF(256) (panics for `0 * 0`, which the C reads past its table for).
pub(crate) fn gf_mul(a: u8, b: u8) -> u8 {
    GF.gf_pow[(GF.gf_log[usize::from(a)] + GF.gf_log[usize::from(b)]) as usize]
}

/// `gf_premul`: precalculate the multiplication table of factor `gn` (`gf_map[gn]`), once;
/// `ENOMEM` when there is no memory for it.
pub(crate) fn gf_premul(gn: u8) -> Result<(), Errno> {
    let slot = &GF_MAP[usize::from(gn)];
    if !slot.load(Ordering::Acquire).is_null() {
        return Ok(());
    }

    let Some(map) = malloc(256, M_DEVBUF, M_ZERO | M_NOWAIT) else {
        return Err(Errno::ENOMEM);
    };

    let lgn = GF.gf_log[usize::from(gn)];
    for i in 0..256 {
        let v = GF.gf_pow[(GF.gf_log[i] + lgn) as usize];
        // SAFETY: `map` is a fresh allocation of 256 bytes and `i < 256`.
        unsafe { map.as_ptr().add(i).write(v) };
    }

    if slot
        .compare_exchange(
            ptr::null_mut(),
            map.as_ptr(),
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        // Another caller installed the same table first.
        free(map, M_DEVBUF, 256);
    }

    Ok(())
}

/// `gf_map[gn]`: the multiplication table of factor `gn`; panics when `gf_premul(gn)` has not
/// made it (the C dereferences NULL).
fn gf_map(gn: u8) -> &'static [u8; 256] {
    let p = GF_MAP[usize::from(gn)].load(Ordering::Acquire);
    if p.is_null() {
        panic(format_args!("gf_map: no table for {}", gn));
    }
    // SAFETY: `gf_premul` installs a 256-byte table, filled before it is published
    // (release/acquire), never written again nor freed.
    unsafe { &*p.cast::<[u8; 256]>() }
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

    /// Reference GF(2^8) multiplication modulo x^8 + x^4 + x^3 + x^2 + 1 (0x11D), bit by bit,
    /// independent of the tables under test.
    fn gmul(mut a: u8, mut b: u8) -> u8 {
        let mut p = 0u8;
        while b != 0 {
            if b & 1 != 0 {
                p ^= a;
            }
            let hi = a & 0x80 != 0;
            a <<= 1;
            if hi {
                a ^= 0x1D;
            }
            b >>= 1;
        }
        p
    }

    /// Reference `2^i` in GF(2^8).
    fn gpow(i: usize) -> u8 {
        (0..i).fold(1u8, |x, _| gmul(x, 2))
    }

    /// Which work unit of a strip an I/O is queued on.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum W {
        /// `wu`: the transfer's own.
        Wu,
        /// `wu_r`: the reads of a write, which run first.
        WuR,
    }

    /// One queued `sr_raid6_addio`.
    #[derive(Clone, Copy, Debug)]
    struct Op {
        wu: W,
        chunk: i64,
        blkno: Daddr,
        len: i64,
        data: usize,
        read: bool,
        freebuf: bool,
        pbuf: Option<usize>,
        qbuf: Option<usize>,
        gn: u8,
    }

    /// A RAID 6 volume in memory: the chunks' bytes and states, an arena of buffers (a
    /// [`Raid6Io::Buf`] is an index into it; `None` once freed) and the queued I/O.
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

        fn n(&self) -> i64 {
            self.chunks.len() as i64
        }

        fn rows(&self) -> usize {
            self.chunks[0].len() / STRIP as usize
        }

        /// The volume's data bytes.
        fn size(&self) -> usize {
            self.rows() * STRIP as usize * (self.chunks.len() - 2)
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
                if let Some(p) = op.pbuf {
                    sr_raid6_xorp(self.buf(p), &got, len);
                }
                if let Some(q) = op.qbuf {
                    sr_raid6_xorq(self.buf(q), &got, len, op.gn);
                }
            } else {
                assert!(
                    matches!(self.status[c], BIOC_SDONLINE | BIOC_SDSCRUB),
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
        fn strip(&self, c: i64, row: usize) -> &[u8] {
            &self.chunks[c as usize][row * STRIP as usize..(row + 1) * STRIP as usize]
        }

        /// Every row's P is the XOR of its data strips and its Q the GF(256) sum of `g^i` times
        /// the data strip of chunk `i`, computed with the reference multiplication.
        fn assert_pq(&self) {
            let nd = self.n() - 2;
            for row in 0..self.rows() {
                let st = Raid6Strip::new((row as i64 * nd) << BITS, STRIP, STRIP, BITS, nd);
                let mut p = vec![0u8; STRIP as usize];
                let mut q = vec![0u8; STRIP as usize];
                for c in 0..self.n() {
                    if c == st.pchunk || c == st.qchunk {
                        continue;
                    }
                    let g = gpow(c as usize);
                    for (j, &d) in self.strip(c, row).iter().enumerate() {
                        p[j] ^= d;
                        q[j] ^= gmul(g, d);
                    }
                }
                assert!(p == self.strip(st.pchunk, row), "row {row}: P is wrong");
                assert!(q == self.strip(st.qchunk, row), "row {row}: Q is wrong");
            }
        }
    }

    impl Raid6Io for Mem {
        type Wu = W;
        type Buf = usize;

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
            pbuf: Option<usize>,
            qbuf: Option<usize>,
            gn: u8,
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
            if qbuf.is_some() {
                gf_premul(gn)?;
            }
            self.ops.push(Op {
                wu,
                chunk,
                blkno,
                len,
                data,
                read: xsflags & SCSI_DATA_IN != 0,
                freebuf: ccbflags & SR_CCBF_FREEBUF != 0,
                pbuf,
                qbuf,
                gn,
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

        fn xorp(&mut self, p: usize, d: usize, len: i64) {
            assert_ne!(p, d);
            let src = self.buf(d)[..len as usize].to_vec();
            sr_raid6_xorp(self.buf(p), &src, len as usize);
        }

        fn xorq(&mut self, q: usize, d: usize, len: i64, gn: u8) {
            assert_ne!(q, d);
            let src = self.buf(d)[..len as usize].to_vec();
            sr_raid6_xorq(self.buf(q), &src, len as usize, gn);
        }
    }

    /// A read or write of `data.len()` bytes at byte `offset` of the volume, strip by strip as
    /// `sr_raid6_rw` does it, each strip's I/O run before the next. Stops at the first strip
    /// that fails.
    fn vol_io(m: &mut Mem, write: bool, offset: usize, data: &mut [u8]) -> Result<(), Errno> {
        let nd = m.n() - 2;
        let mut lbaoffs = offset as i64;
        let mut datalen = data.len() as i64;
        let mut done = 0usize;
        while datalen != 0 {
            let st = Raid6Strip::new(lbaoffs, datalen, STRIP, BITS, nd);
            let len = st.length as usize;
            let piece = m.add(data[done..done + len].to_vec());
            let (wu_r, flags) = if write {
                (Some(W::WuR), SCSI_DATA_OUT)
            } else {
                (None, SCSI_DATA_IN)
            };
            if let Err(e) = sr_raid6_strip(m, W::Wu, wu_r, flags, &st, nd, piece) {
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
    fn gf_tables_match_the_field() {
        // gf_init: g^i, a doubled table, zero beyond
        for i in 0..=510 {
            assert_eq!(GF.gf_pow[i], gpow(i % 255), "gf_pow[{i}]");
        }
        assert!(GF.gf_pow[511..].iter().all(|&x| x == 0));
        assert_eq!(GF.gf_log[0], 512);
        assert_eq!(GF.gf_log[1], 255);
        for i in 1..255 {
            assert_eq!(GF.gf_log[usize::from(gpow(i))], i as i32);
        }

        for a in 1..=255u8 {
            assert_eq!(gf_mul(a, gf_inv(a)), 1, "inverse of {a}");
            assert_eq!(gmul(a, gf_inv(a)), 1, "inverse of {a}");
            assert_eq!(gf_mul(a, 0), 0);
            assert_eq!(gf_mul(0, a), 0);
            for b in 1..=255u8 {
                assert_eq!(gf_mul(a, b), gmul(a, b), "{a} * {b}");
            }
        }
    }

    #[test]
    fn premul_tables_and_xorq() {
        let _g = setup_real_memory();
        for gn in [1u8, 2, 3, 0x1d, 0x8e, 0xff] {
            gf_premul(gn).unwrap();
            gf_premul(gn).unwrap(); // once only
            let map = gf_map(gn);
            for x in 0..=255u8 {
                assert_eq!(map[usize::from(x)], gmul(gn, x), "{gn} * {x}");
            }
        }

        // q ^= gn * d, in whole 32-bit words as the C
        let d = bytes(5, 7);
        let mut q = vec![0x55u8; 7];
        sr_raid6_xorq(&mut q, &d, 7, 0x8e);
        for j in 0..4 {
            assert_eq!(q[j], 0x55 ^ gmul(0x8e, d[j]));
        }
        assert_eq!(&q[4..], &[0x55; 3]);
        let mut p = vec![0x0fu8; 6];
        sr_raid6_xorp(&mut p, &[0xff; 6], 6);
        assert_eq!(p, [0xf0, 0xf0, 0xf0, 0xf0, 0x0f, 0x0f]);
    }

    #[test]
    fn strip_map_rotates_p_and_q() {
        for n in 4..=8i64 {
            let nd = n - 2;
            for row in 0..3 * n {
                let mut seen = vec![false; n as usize];
                let mut last = -1;
                for k in 0..nd {
                    let strip_no = row * nd + k;
                    let st = Raid6Strip::new(strip_no << BITS, STRIP, STRIP, BITS, nd);
                    assert_eq!(st.strip_no, strip_no);
                    assert_eq!(st.lba, row * (STRIP / DEV_BSIZE as i64));
                    // Q starts on the last chunk and moves left one chunk per row, P just left
                    // of it (wrapping to the last chunk)
                    assert_eq!(st.qchunk, (n - 1) - row % n);
                    assert_eq!(st.pchunk, (st.qchunk + n - 1) % n);
                    // data fills the other chunks left to right
                    assert!(st.chunk > last && st.chunk < n);
                    assert!(st.chunk != st.pchunk && st.chunk != st.qchunk);
                    last = st.chunk;
                    seen[st.chunk as usize] = true;
                    if k == 0 {
                        seen[st.pchunk as usize] = true;
                        seen[st.qchunk as usize] = true;
                    }
                }
                assert!(seen.iter().all(|&s| s), "n {n} row {row}");
            }
        }
        // part of a strip
        let st = Raid6Strip::new((3 << BITS) + 512, 10_000, STRIP, BITS, 2);
        assert_eq!(
            (st.length, st.lba),
            (STRIP - 512, (STRIP + 512) / DEV_BSIZE as i64)
        );
    }

    #[test]
    fn sizes_states_and_init() {
        let strip = MAXPHYS as u32;
        let blocks = i64::from(strip) / DEV_BSIZE as i64;
        assert_eq!(raid6_volume_size(10 * blocks + 7, strip, 4), 20 * blocks);
        assert_eq!(raid6_volume_size(10 * blocks, strip, 6), 40 * blocks);

        let st = |online: i64, rebuild: i64, offline: i64| {
            let mut s = [0i64; SR_MAX_STATES];
            s[BIOC_SDONLINE as usize] = online;
            s[BIOC_SDREBUILD as usize] = rebuild;
            s[BIOC_SDOFFLINE as usize] = offline;
            raid6_vol_state(&s, online + rebuild + offline)
        };
        assert_eq!(st(5, 0, 0), Some(BIOC_SVONLINE));
        assert_eq!(st(4, 0, 1), Some(BIOC_SVDEGRADED));
        assert_eq!(st(3, 0, 2), Some(BIOC_SVDEGRADED));
        assert_eq!(st(3, 1, 1), Some(BIOC_SVREBUILD));
        assert_eq!(st(2, 0, 3), Some(BIOC_SVOFFLINE));
        assert!(raid6_chunk_state_ok(BIOC_SDONLINE, BIOC_SDOFFLINE));
        assert!(raid6_chunk_state_ok(BIOC_SDOFFLINE, BIOC_SDREBUILD));
        assert!(!raid6_chunk_state_ok(BIOC_SDOFFLINE, BIOC_SDONLINE));
        assert!(!raid6_chunk_state_ok(BIOC_SDHOTSPARE, BIOC_SDONLINE));
        assert!(raid6_vol_state_ok(BIOC_SVDEGRADED, BIOC_SVDEGRADED));
        assert!(!raid6_vol_state_ok(BIOC_SVDEGRADED, BIOC_SVONLINE));
        assert!(!raid6_vol_state_ok(BIOC_SVOFFLINE, BIOC_SVOFFLINE));

        let _g = setup_real_memory();
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
        let n = 5;
        sd.sd_meta().ssdi().ssd_chunk_no.set(n);
        sd.sd_vol.sv_chunks_alloc(n as usize, M_WAITOK).unwrap();
        for i in 0..n as usize {
            // SAFETY: zeroed (`SrZeroed`) and leaked.
            let c: &'static SrChunk = unsafe { sr_malloc::<SrChunk>(M_WAITOK).unwrap().as_ref() };
            c.src_meta.scm_status.set(BIOC_SDONLINE as u32);
            sd.sd_vol.set_sv_chunk(i, Some(c));
        }

        sr_raid6_discipline_init(sd);
        assert_eq!(sd.sd_type.get(), SR_MD_RAID6);
        assert_eq!(&sd.sd_name.get()[..6], b"RAID 6");
        assert_eq!(sr_raid6_openings(sd), SR_RAID6_NOWU as i32 / 2);
        assert!(sd.sd_capabilities.get() & SR_CAP_REBUILD == 0);
        sd.sd_meta().ssdi().ssd_strip_size.set(strip);
        sr_raid6_init(sd).unwrap();
        assert_eq!(sd.sd_max_ccb_per_wu.get(), 10);
        sd.sd_meta().ssdi().ssd_strip_size.set(1000);
        assert_eq!(sr_raid6_init(sd), Err(Errno::EINVAL));

        sd.sd_vol_status.set(BIOC_SVONLINE);
        for (c, want) in [
            (1, BIOC_SVDEGRADED),
            (3, BIOC_SVDEGRADED),
            (4, BIOC_SVOFFLINE),
        ] {
            sd.sd_vol
                .sv_chunk(c)
                .src_meta
                .scm_status
                .set(BIOC_SDOFFLINE as u32);
            sr_raid6_set_vol_state(sd);
            assert_eq!(sd.sd_vol_status.get(), want);
        }
    }

    #[test]
    fn writes_compute_p_and_q() {
        let _g = setup_real_memory();
        for n in 4..=7 {
            let mut m = Mem::new(n, 2 * n);
            let mut expect = bytes(n as u32, m.size());
            vol_io(&mut m, true, 0, &mut expect.clone()).unwrap();
            m.assert_pq();
            // read-modify-write of pieces of strips (whole sectors) keeps both parities
            for (k, (off, len)) in [(512, 9216), (4096 * 5 - 512, 1024), (1536, 512)]
                .iter()
                .enumerate()
            {
                let mut piece = bytes(1000 + k as u32, *len);
                vol_io(&mut m, true, *off, &mut piece).unwrap();
                expect[*off..*off + *len].copy_from_slice(&piece);
            }
            assert_eq!(m.live(), 0, "leaked blocks");
            m.assert_pq();
            assert_reads(&mut m, &expect);
        }
    }

    #[test]
    fn one_or_two_missing_chunks_are_recovered() {
        // Over the rows, two missing chunks are every pair of roles: two data strips (Dx+Dy, from
        // P and Q), data and P (from Q), data and Q (from P), P and Q (plain reads).
        let _g = setup_real_memory();
        for n in 4..=6 {
            let mut m = Mem::new(n, 2 * n);
            let expect = bytes(17 * n as u32, m.size());
            vol_io(&mut m, true, 0, &mut expect.clone()).unwrap();
            for a in 0..n {
                m.status[a] = BIOC_SDOFFLINE;
                assert_reads(&mut m, &expect);
                for b in a + 1..n {
                    m.status[b] = BIOC_SDREBUILD;
                    assert_reads(&mut m, &expect);
                    m.status[b] = BIOC_SDONLINE;
                }
                m.status[a] = BIOC_SDONLINE;
            }
        }
    }

    #[test]
    fn degraded_writes() {
        // With chunk k offline, a strip write fails when k holds the strip, P or Q, and else
        // updates P and Q from the old data, so the volume stays consistent and readable.
        let _g = setup_real_memory();
        for n in 4..=6 {
            for k in 0..n {
                let mut m = Mem::new(n, 2 * n);
                let mut expect = bytes(5 * n as u32 + k as u32, m.size());
                vol_io(&mut m, true, 0, &mut expect.clone()).unwrap();
                m.status[k] = BIOC_SDOFFLINE;
                let nd = n as i64 - 2;
                let new = bytes(200 + k as u32, m.size());
                for s in 0..m.size() / STRIP as usize {
                    let st = Raid6Strip::new((s as i64) << BITS, STRIP, STRIP, BITS, nd);
                    let hit = [st.chunk, st.pchunk, st.qchunk].contains(&(k as i64));
                    let range = s * STRIP as usize..(s + 1) * STRIP as usize;
                    let mut piece = new[range.clone()].to_vec();
                    let r = vol_io(&mut m, true, range.start, &mut piece);
                    assert_eq!(r.is_err(), hit, "n {n} k {k} strip {s}");
                    if !hit {
                        expect[range].copy_from_slice(&piece);
                    }
                }
                assert_eq!(m.live(), 0, "leaked blocks");
                assert_reads(&mut m, &expect);
                m.status[k] = BIOC_SDONLINE;
                m.assert_pq();
                assert_reads(&mut m, &expect);
            }
        }
    }

    #[test]
    fn failed_queueing_frees_p_and_q() {
        // Fail each of the six addio calls of a strip write in turn: the write fails and no
        // block is left behind.
        let _g = setup_real_memory();
        for k in 0..8 {
            let mut m = Mem::new(5, 3);
            m.fail_in = Some(k);
            let mut piece = bytes(k as u32, STRIP as usize);
            let r = vol_io(&mut m, true, STRIP as usize, &mut piece);
            assert_eq!(r.is_ok(), k >= 6, "addio {k} failed");
            assert_eq!(m.live(), 0, "leaked blocks (addio {k} failed)");
        }
    }
}
/* </TESTS> */
