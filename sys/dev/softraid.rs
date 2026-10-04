/* $OpenBSD: softraid.c,v 1.440 2026/09/29 22:49:58 krw Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2007, 2008, 2009 Marco Peereboom <marco@peereboom.us>
 * Copyright (c) 2008 Chris Kuethe <ckuethe@openbsd.org>
 * Copyright (c) 2009 Joel Sing <jsing@openbsd.org>
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

//! `softraid.c`: softraid(4), the software RAID framework. Volumes ("disciplines": RAID 0,
//! 1, 5, 6, concat, crypto, RAID 1C) are made of chunks (disk partitions of type `RAID`),
//! carry their metadata on every chunk, and appear as SCSI disks (`sd(4)`) on softraid's own
//! `scsibus`. This file holds the metadata handling, the work units and ccbs the disciplines
//! build their I/O from, the SCSI emulation defaults, and (phase 2 of the port) the bio(4)
//! ioctls, boot-time assembly, hotspares, rebuild and sensors.
//!
//! Upstream: sys/dev/softraid.c @ 3ce1f3f79392
//!
//! A discipline (`dev/softraid_*.rs`) is set up by [`sr_discipline_init`], which installs
//! the defaults below in the `sd_*` hooks of [`SrDiscipline`] and calls the discipline's
//! `sr_*_discipline_init`, which overrides some of them (`sd.sd_scsi_rw.set(Some(..))`).
//! A SCSI command arrives as a work unit ([`SrWorkunit`], the volume's `scsi_iopool`
//! opening); the discipline's `sd_scsi_rw` splits it into ccbs ([`sr_ccb_rw`],
//! [`sr_wu_enqueue_ccb`]) and schedules it ([`sr_schedule_wu`]); each ccb's completion
//! ([`sr_raid_intr`]) counts towards the work unit, whose last one queues
//! `sr_wu_done_callback` on the discipline's task queue, which completes the transfer.
//!
//! ## Deviations
//! - `SR_DEBUG` (and with it `SR_FANCY_STATS`, `sr_print_stats`, `sr_meta_print`,
//!   `sr_dump_block`, `sr_dump_mem`, `sr_checksum_print`'s callers) is not configured; the
//!   `DNPRINTF` calls are comments.
//! - Discipline initialisation: until each `dev/softraid_*.rs` exists, its arm of
//!   `sr_discipline_init` reports `unported!("softraid_<x>.c")` and fails (the discipline
//!   ports replace the arm by the one-line call).
//! - Hooks and helpers that return the C's 0/1 return `Result<(), Errno>`, `Err(EIO)` for
//!   the C's 1 (`softraidvar.rs`); `sr_validate_io` returns the block number;
//!   `sr_validate_stripsize` returns `Option` (`None` for the C's -1; a zero strip size,
//!   which loops forever in C, is `None`); `sr_block_get` returns `Option<NonNull<u8>>`.
//! - Metadata buffers are cell views (`&[Cell<u8>]`, `SrMetaView` structures); `sr_rw`
//!   reads into and writes from such a view. Its `struct buf` (a local in C) is a `bufpool`
//!   item, as `physio` makes one, because `VOP_STRATEGY`/`biowait` take `&'static Buf`;
//!   `dma_alloc` is `DmaBuf` (`kern/dma_alloc.c` is not ported: `malloc(9)` memory).
//! - `sr_meta_save`'s fake work unit and `sr_rebuild`'s two `struct scsi_xfer` locals are
//!   allocated (`sr_malloc`, `scsi_xfer_pool`), for the same reason.
//! - `sr_ccb_free` clears `sd_ccb` after freeing it (the C leaves the pointer dangling).
//! - `sr_hotplug_register`/`unregister` take a typed callback ([`SrHotplugFn`]) instead of a
//!   `void *`; the comparison is `ptr::fn_addr_eq`.
//! - The `CRYPTO` arms (`'C'`, `0x1C`) are always compiled: GENERIC has `option CRYPTO`.
//!
//! Status: wip (phase 1: the discipline-facing part). Not yet in this file: the boot
//! probe and assembly, `sr_map_root`, attach/detach, the SCSI adapter, the bio ioctls,
//! hotspares (`sr_hotspare_rebuild` reports itself), `sr_rebuild_init`, roaming, the
//! discipline shutdown, sensors, `sr_quiesce`/`sr_shutdown`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::AtomicPtr;

use libkern::strlcpy;

use crate::crypto::md5::{MD5_DIGEST_LENGTH, MD5Final, MD5Init, MD5Update, Md5Ctx};
use crate::dev::bio::bio_status;
use crate::dev::biovar::{
    BIO_MSG_ERROR, BIO_MSG_INFO, BIO_MSG_WARN, BIOC_SDINVALID, BIOC_SDOFFLINE, BIOC_SDONLINE,
    BIOC_SDREBUILD, BIOC_SVINVALID, BIOC_SVOFFLINE, BIOC_SVONLINE,
};
use crate::dev::rnd::arc4random_buf;
use crate::dev::softraidvar::*;
use crate::kassert;
use crate::kern::kern_kthread::kthread_exit;
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_rwlock::rw_assert_wrlock;
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_task::{task_add, task_set};
use crate::kern::subr_disk::findblkname;
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::kern::subr_prf::{Str, panic, printf, snprintf};
use crate::kern::vfs_bio::{BUFPOOL, biowait};
use crate::kern::vfs_subr::{bdevvp, vput};
use crate::kern::vfs_vops::{VOP_IOCTL, VOP_OPEN, VOP_STRATEGY};
use crate::machine::cpu::curproc;
use crate::machine::intr::{IPL_BIO, splassert, splbio, splx};
use crate::queue_adapter;
use crate::scsi::scsi_all::{
    SI_EVPD, SID_CmdQue, SID_SCSI2_ALEN, SID_SCSI2_RESPONSE, SKEY_HARDWARE_ERROR,
    SKEY_ILLEGAL_REQUEST, SKEY_NOT_READY, SSD_ERRCODE_CURRENT, SSD_ERRCODE_VALID, ScsiGeneric,
    ScsiInquiry, ScsiInquiryData, ScsiReadCapData, ScsiReadCapData16, ScsiWire, T_DIRECT,
};
use crate::scsi::scsi_base::{
    SCSI_XFER_POOL, scsi_copy_internal_data, scsi_done, scsi_io_get, scsi_io_put,
};
use crate::scsi::scsi_disk::{
    READ_16, READ_CAPACITY, READ_CAPACITY_16, ScsiRw, ScsiRw10, ScsiRw16, WRITE_16,
};
use crate::scsi::scsiconf::{
    _3btol, _4btol, _8btol, _lto4b, _lto8b, DmaBuf, SCSI_DATA_IN, SCSI_DATA_OUT, SCSI_REV_2,
    ScsiIo, ScsiXfer, XS_DRIVER_STUFFUP, XS_NOERROR,
};
use crate::sys::buf::{B_CALL, B_ERROR, B_PHYS, B_READ, B_WRITE, Buf};
use crate::sys::disk::Disk;
use crate::sys::disklabel::{
    Disklabel, FS_RAID, diskpart, diskunit, dl_getpsize, dl_partnum2name, dl_sectoblk,
};
use crate::sys::dkio::DIOCGDINFO;
use crate::sys::errno::Errno;
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_WAITOK, M_ZERO};
use crate::sys::param::{DEV_BSHIFT, DEV_BSIZE, MAXPHYS, NODEV, PRIBIO, PWAIT};
use crate::sys::pool::{PR_WAITOK, PR_ZERO};
use crate::sys::queue::{SlistEntry, SlistHead};
use crate::sys::systm::INFSLP;
use crate::sys::task::SYSTQ;
use crate::sys::time::{msec_to_nsec, sec_to_nsec};
use crate::sys::types::{Daddr, Dev, major};
use crate::sys::ucred::NOCRED;
use crate::unported;

/// `SR_META_NOTCLAIMED`.
pub const SR_META_NOTCLAIMED: i32 = 0;
/// `SR_META_CLAIMED`.
pub const SR_META_CLAIMED: i32 = 1;

/// The size of a metadata area (`SR_META_SIZE * DEV_BSIZE`).
pub const SR_META_BYTES: usize = SR_META_SIZE * DEV_BSIZE;

/// `struct sr_hotplug_list`: a discipline's hotplug callback.
pub struct SrHotplugList {
    /// `sh_hotplug`.
    sh_hotplug: Cell<Option<SrHotplugFn>>,
    /// `sh_sd`.
    sh_sd: Cell<Option<&'static SrDiscipline>>,
    /// `shl_link`.
    shl_link: SlistEntry<SrHotplugList>,
}

// SAFETY: `Option`s of `fn` and references (None) and a list entry; no `Drop`.
unsafe impl SrZeroed for SrHotplugList {}

queue_adapter!(
    /// `SLIST_HEAD(sr_hotplug_list_head, sr_hotplug_list)`, through `shl_link`.
    pub SrHotplugLink: SrHotplugList, shl_link => SlistEntry<SrHotplugList>
);

/// `sr_hotplug_callbacks`, made `Sync`.
pub struct SrHotplugListHead(SlistHead<SrHotplugLink>);

// SAFETY: the list changes under the kernel lock (bio ioctls, attach); one CPU.
unsafe impl Sync for SrHotplugListHead {}

/// The metadata reader and writer of a format (`smd_read`, `smd_write`): the metadata area
/// is `SR_META_BYTES` cells; the last argument is the foreign metadata (`void *`).
pub type SmdRwFn =
    fn(sd: &SrDiscipline, dev: Dev, md: &[Cell<u8>], fm: *mut c_void) -> Result<(), Errno>;

/// `smd_probe`: the `SR_META_F_*` of a chunk.
pub type SmdProbeFn = fn(sc: &SrSoftc, ch_entry: &SrChunk) -> i32;
/// `smd_attach`.
pub type SmdAttachFn = fn(sd: &'static SrDiscipline, force: i32) -> Result<(), Errno>;
/// `smd_detach`.
pub type SmdDetachFn = fn(sd: &'static SrDiscipline) -> Result<(), Errno>;
/// `smd_validate`: checks and translates foreign metadata.
pub type SmdValidateFn =
    fn(sd: &SrDiscipline, sm: &SrMetadata, fm: *mut c_void) -> Result<(), Errno>;

/// `struct sr_meta_driver`: a metadata format. The metadata driver should remain stateless.
pub struct SrMetaDriver {
    /// `smd_offset`: metadata location.
    pub smd_offset: Daddr,
    /// `smd_size`: size of metadata.
    pub smd_size: usize,
    /// `smd_probe`: `SR_META_F_*` of the chunk.
    pub smd_probe: Option<SmdProbeFn>,
    /// `smd_attach`.
    pub smd_attach: Option<SmdAttachFn>,
    /// `smd_detach`.
    pub smd_detach: Option<SmdDetachFn>,
    /// `smd_read`.
    pub smd_read: Option<SmdRwFn>,
    /// `smd_write`.
    pub smd_write: Option<SmdRwFn>,
    /// `smd_validate`.
    pub smd_validate: Option<SmdValidateFn>,
}

/// `smd[]`: the metadata formats, terminated by an empty entry.
pub static SMD: [SrMetaDriver; 2] = [
    SrMetaDriver {
        smd_offset: SR_META_OFFSET,
        smd_size: SR_META_BYTES,
        smd_probe: Some(sr_meta_native_probe),
        smd_attach: Some(sr_meta_native_attach),
        smd_detach: None,
        smd_read: Some(sr_meta_native_read),
        smd_write: Some(sr_meta_native_write),
        smd_validate: None,
    },
    SrMetaDriver {
        smd_offset: 0,
        smd_size: 0,
        smd_probe: None,
        smd_attach: None,
        smd_detach: None,
        smd_read: None,
        smd_write: None,
        smd_validate: None,
    },
];

/// `softraid0`: the softc, once `sr_attach` has run.
pub static SOFTRAID0: AtomicPtr<SrSoftc> = AtomicPtr::new(ptr::null_mut());

/// `sr_hotplug_callbacks`.
pub static SR_HOTPLUG_CALLBACKS: SrHotplugListHead = SrHotplugListHead(SlistHead::new());

/// `&smd[sd->sd_meta_type]`.
fn smd(sd: &SrDiscipline) -> &'static SrMetaDriver {
    let t = sd.sd_meta_type.get();
    match usize::try_from(t).ok().and_then(|i| SMD.get(i)) {
        Some(s) => s,
        None => panic(format_args!("softraid: invalid metadata type {}", t)),
    }
}

/// A `malloc`ed, zeroed metadata area of `SR_META_BYTES` (or `len`) bytes, freed on drop:
/// the C's `m = malloc(SR_META_SIZE * DEV_BSIZE, M_DEVBUF, M_ZERO | ...)` scratch buffers.
pub struct SrMetaBuf {
    ptr: NonNull<u8>,
    len: usize,
}

impl SrMetaBuf {
    /// A zeroed area of `len` bytes; `None` when `M_NOWAIT` finds no memory.
    pub fn new(len: usize, flags: i32) -> Option<Self> {
        let ptr = malloc(len.max(1), M_DEVBUF, flags | M_ZERO)?;
        Some(Self { ptr, len })
    }

    /// The area as cells.
    pub fn cells(&self) -> &[Cell<u8>] {
        // SAFETY: `len` bytes at `ptr` are this buffer's own allocation, initialised (zeroed)
        // and borrowed through `self`; bytes are valid as cells.
        unsafe { core::slice::from_raw_parts(self.ptr.as_ptr().cast::<Cell<u8>>(), self.len) }
    }

    /// The area as `struct sr_metadata` (it is at least that large).
    pub fn md(&self) -> &SrMetadata {
        match SrMetadata::view(self.cells()) {
            Some(m) => m,
            None => panic(format_args!("sr_meta_buf too small")),
        }
    }
}

impl Drop for SrMetaBuf {
    fn drop(&mut self) {
        free(self.ptr, M_DEVBUF, self.len.max(1));
    }
}

/// `(struct sr_meta_chunk *)(sm + 1) + i`: chunk `i`'s metadata in a metadata area.
pub fn sr_meta_chunk_at(area: &[Cell<u8>], i: usize) -> Option<&SrMetaChunk> {
    sr_view::<SrMetaChunk>(area, size_of::<SrMetadata>() + i * size_of::<SrMetaChunk>())
}

/// The bytes of `b` up to its first NUL.
fn cstr(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

/// `strlcpy` into a byte-array cell.
pub fn sr_strlcpy_cell<const N: usize>(dst: &Cell<[u8; N]>, src: &[u8]) {
    let mut b = [0u8; N];
    let _ = strlcpy(&mut b, src);
    dst.set(b);
}

/// `strncmp(a, b, n) == 0` of two NUL-padded names of at most `n` bytes.
pub fn sr_name_eq(a: &[u8], b: &[u8]) -> bool {
    cstr(a) == cstr(b)
}

/// `sr_meta_attach`: makes the in-memory metadata of a discipline whose chunks
/// `sr_meta_probe` found, attaches the metadata format, and orders the chunks by id.
pub fn sr_meta_attach(sd: &'static SrDiscipline, chunk_no: i32, force: i32) -> Result<(), Errno> {
    let sc = sd.sd_sc();

    // DNPRINTF(SR_D_META, "sr_meta_attach(%d)")

    // in memory copy of metadata
    let Some(meta) = sr_malloc_size::<SrMetadata>(SR_META_BYTES, M_NOWAIT) else {
        sr_error(sc, format_args!("could not allocate memory for metadata"));
        return Err(Errno::EIO);
    };
    sd.sd_meta.set(Some(meta));

    if sd.sd_meta_type.get() != SR_META_F_NATIVE {
        // in memory copy of foreign metadata
        let Some(fm) = malloc(smd(sd).smd_size.max(1), M_DEVBUF, M_ZERO | M_NOWAIT) else {
            // unwind frees sd_meta
            sr_error(
                sc,
                format_args!("could not allocate memory for foreign metadata"),
            );
            return Err(Errno::EIO);
        };
        sd.sd_meta_foreign.set(fm.as_ptr().cast());
    }

    // we have a valid list now create an array index
    let cl = &sd.sd_vol.sv_chunk_list;
    let chunk_no = usize::try_from(chunk_no).unwrap_or(0);
    sd.sd_vol.sv_chunks_alloc(chunk_no, M_WAITOK)?;

    // fill out chunk array
    for (i, ch_entry) in cl.iter().enumerate() {
        sd.sd_vol.set_sv_chunk(i, Some(ch_entry));
    }

    // attach metadata
    let Some(attach) = smd(sd).smd_attach else {
        return Err(Errno::EIO);
    };
    attach(sd, force)?;

    // Force chunks into correct order now that metadata is attached.
    cl.init();
    for i in 0..chunk_no {
        let ch_entry = sd.sd_vol.sv_chunk(i);
        let id = ch_entry.src_meta.scmi().scm_chunk_id.get();
        let mut chunk2: Option<&SrChunk> = None;
        for chunk1 in cl.iter() {
            if chunk1.src_meta.scmi().scm_chunk_id.get() > id {
                break;
            }
            chunk2 = Some(chunk1);
        }
        // SAFETY: the list was emptied above and each chunk is inserted once; chunks live
        // until `sr_chunks_unwind` frees them after unlinking.
        unsafe {
            match chunk2 {
                None => cl.insert_head(ch_entry),
                Some(c2) => SrChunkHead::insert_after(c2, ch_entry),
            }
        }
    }
    for (i, ch_entry) in cl.iter().enumerate() {
        sd.sd_vol.set_sv_chunk(i, Some(ch_entry));
    }

    Ok(())
}

/// `sr_meta_probe`: opens the `no_chunk` devices of `dt` as the discipline's chunks (in the
/// user's order; `NODEV` is an offline chunk) and finds the metadata format they share.
/// Returns that `SR_META_F_*`, or `SR_META_F_INVALID`.
pub fn sr_meta_probe(sd: &'static SrDiscipline, dt: &[Dev]) -> i32 {
    let sc = sd.sd_sc();

    // DNPRINTF(SR_D_META, "sr_meta_probe(%d)")

    if dt.is_empty() {
        return SR_META_F_INVALID;
    }

    let cl = &sd.sd_vol.sv_chunk_list;
    let mut ch_prev: Option<&'static SrChunk> = None;
    let mut prevf = SR_META_F_INVALID;

    for &dev in dt {
        let Some(ch) = sr_malloc::<SrChunk>(M_WAITOK) else {
            return SR_META_F_INVALID;
        };
        // SAFETY: a zeroed chunk (`SrZeroed`), which lives until `sr_chunks_unwind`.
        let ch_entry: &'static SrChunk = unsafe { ch.as_ref() };
        // keep disks in user supplied order
        // SAFETY: a new chunk in no list; `ch_prev` is linked in `cl`.
        unsafe {
            match ch_prev {
                Some(prev) => SrChunkHead::insert_after(prev, ch_entry),
                None => cl.insert_head(ch_entry),
            }
        }
        ch_prev = Some(ch_entry);
        ch_entry.src_dev_mm.set(dev);

        if dev == NODEV {
            ch_entry.src_meta.scm_status.set(BIOC_SDOFFLINE as u32);
            continue;
        }
        let mut devname = [0u8; 32];
        sr_meta_getdevname(sc, dev, &mut devname);
        let vn = match bdevvp(dev) {
            Ok(Some(vn)) => vn,
            _ => {
                sr_error(sc, format_args!("sr_meta_probe: cannot allocate vnode"));
                return SR_META_F_INVALID;
            }
        };

        // XXX leaving dev open for now; move this to attach and figure out the open/close
        // dance for unwind.
        let Some(p) = curproc() else {
            vput(vn);
            return SR_META_F_INVALID;
        };
        if VOP_OPEN(vn, FREAD | FWRITE, NOCRED, p).is_err() {
            // DNPRINTF(SR_D_META, "sr_meta_probe can't open %s")
            vput(vn);
            return SR_META_F_INVALID;
        }

        sr_strlcpy_cell(&ch_entry.src_devname, &devname);
        ch_entry.src_vn.set(Some(vn));

        // determine if this is a device we understand
        let mut found = SR_META_F_INVALID;
        for s in SMD.iter() {
            let Some(probe) = s.smd_probe else {
                break;
            };
            let t = probe(sc, ch_entry);
            if t != SR_META_F_INVALID {
                found = t;
                break;
            }
        }

        if found == SR_META_F_INVALID {
            return SR_META_F_INVALID;
        }
        if prevf == SR_META_F_INVALID {
            prevf = found;
        }
        if prevf != found {
            // DNPRINTF(SR_D_META, "prevf != found")
            return SR_META_F_INVALID;
        }
    }

    prevf
}

/// `sr_meta_getdevname`: the device's name (`sd0a`) into `buf`, NUL-terminated; untouched
/// when the major has no block device name.
pub fn sr_meta_getdevname(_sc: &SrSoftc, dev: Dev, buf: &mut [u8]) {
    // DNPRINTF(SR_D_META, "sr_meta_getdevname")

    if buf.is_empty() {
        return;
    }

    let maj = major(dev) as i32;
    let part = diskpart(dev);
    let unit = diskunit(dev);

    let Some(name) = findblkname(maj) else {
        return;
    };

    let partname = dl_partnum2name(part as usize).map_or('?', char::from);
    let _ = snprintf(buf, format_args!("{}{}{}", Str(name), unit, partname));
}

/// `sr_rw`: reads (`B_READ`) or writes (`B_WRITE`) `buf.len()` bytes of `dev` at block
/// `blkno`, `MAXPHYS` at a time through a DMA buffer.
pub fn sr_rw(
    sc: &SrSoftc,
    dev: Dev,
    buf: &[Cell<u8>],
    mut blkno: Daddr,
    flags: i64,
) -> Result<(), Errno> {
    // DNPRINTF(SR_D_MISC, "sr_rw")

    let mut size = buf.len();
    let dma_bufsize = size.min(MAXPHYS);
    let Some(mut dma_buf) = DmaBuf::new(dma_bufsize, M_WAITOK) else {
        return Err(Errno::ENOMEM);
    };

    let vp = match bdevvp(dev) {
        Ok(Some(vp)) => vp,
        _ => {
            printf(format_args!(
                "{}: sr_rw: failed to allocate vnode\n",
                DEVNAME(sc)
            ));
            return Err(Errno::EIO);
        }
    };

    // The C's `struct buf b` on the stack: a bufpool item, as `physio` makes one.
    let s = splbio();
    let Some(mem) = pool_get(&BUFPOOL, PR_WAITOK | PR_ZERO) else {
        panic(format_args!("sr_rw: pool_get"));
    };
    splx(s);
    let item = mem.cast::<Buf>();

    let mut off = 0;
    let mut rv = Ok(());
    while size > 0 {
        // DNPRINTF(SR_D_MISC, "dma_buf %p, size %zu, blkno %lld")

        let bufsize = size.min(MAXPHYS);
        let chunk = &buf[off..off + bufsize];
        let data = dma_buf.bytes();
        if flags == B_WRITE {
            cells_read(&mut data[..bufsize], chunk);
        }

        // SAFETY: a suitably aligned `bufpool` item of `size_of::<Buf>()` bytes, rewritten
        // (`bzero(&b, sizeof(b))`) while nothing else references it: the previous
        // iteration's I/O is done (`biowait`).
        unsafe { item.as_ptr().write(Buf::new()) };
        // SAFETY: initialised above; it stays allocated until the `pool_put` below.
        let b: &'static Buf = unsafe { item.as_ref() };
        b.b_flags.set(flags | B_PHYS);
        b.b_proc.set(curproc().map_or(ptr::null(), ptr::from_ref));
        b.b_dev.set(dev);
        b.b_iodone.set(None);
        b.b_error.set(None);
        b.b_blkno.set(blkno);
        b.b_data.set(data.as_mut_ptr());
        b.b_bcount.set(bufsize as i64);
        b.b_bufsize.set(bufsize as i64);
        b.b_resid.set(bufsize);
        b.b_vp.set(Some(vp));

        if !b.isset(B_READ) {
            let s = splbio();
            vp.v_numoutput.set(vp.v_numoutput.get() + 1);
            splx(s);
        }

        let _ = VOP_STRATEGY(vp, b);
        let _ = biowait(b);

        if b.isset(B_ERROR) {
            printf(format_args!(
                "{}: I/O error {} on dev {:#x} at block {}\n",
                DEVNAME(sc),
                b.b_error.get().map_or(0, |e| e as i32),
                dev,
                b.b_blkno.get()
            ));
            rv = Err(Errno::EIO);
            break;
        }

        if flags == B_READ {
            cells_write(chunk, &dma_buf.bytes()[..bufsize]);
        }

        size -= bufsize;
        off += bufsize;
        blkno += bufsize.div_ceil(DEV_BSIZE) as Daddr;
    }

    let s = splbio();
    pool_put(&BUFPOOL, mem);
    splx(s);

    vput(vp);

    rv
}

/// `sr_meta_rw`: reads or writes a metadata area at `SR_META_OFFSET` of `dev`.
pub fn sr_meta_rw(sd: &SrDiscipline, dev: Dev, md: &[Cell<u8>], flags: i64) -> Result<(), Errno> {
    // DNPRINTF(SR_D_META, "sr_meta_rw")

    if md.len() < SR_META_BYTES {
        printf(format_args!(
            "{}: sr_meta_rw: invalid metadata pointer\n",
            DEVNAME(sd.sd_sc())
        ));
        return Err(Errno::EIO);
    }

    sr_rw(sd.sd_sc(), dev, &md[..SR_META_BYTES], SR_META_OFFSET, flags)
}

/// `sr_meta_clear`: zeroes the metadata on every chunk (native metadata only).
pub fn sr_meta_clear(sd: &SrDiscipline) -> Result<(), Errno> {
    let sc = sd.sd_sc();
    let cl = &sd.sd_vol.sv_chunk_list;

    // DNPRINTF(SR_D_META, "sr_meta_clear")

    if sd.sd_meta_type.get() != SR_META_F_NATIVE {
        sr_error(sc, format_args!("cannot clear foreign metadata"));
        return Err(Errno::EIO);
    }

    let Some(m) = SrMetaBuf::new(SR_META_BYTES, M_WAITOK) else {
        return Err(Errno::ENOMEM);
    };
    for ch_entry in cl.iter() {
        if sr_meta_native_write(sd, ch_entry.src_dev_mm.get(), m.cells(), ptr::null_mut()).is_err()
        {
            // XXX mark disk offline
            // DNPRINTF(SR_D_META, "sr_meta_clear failed to clear %s")
            continue;
        }
        ch_entry.src_meta.bzero();
    }

    sd.sd_meta_cells().iter().for_each(|c| c.set(0));

    Ok(())
}

/// `sr_meta_init`: fills a new volume's metadata (level `level`, `no_chunk` chunks) and the
/// chunks', computes the chunk sizes and the sector size.
pub fn sr_meta_init(sd: &SrDiscipline, level: i32, no_chunk: i32) {
    let sc = sd.sd_sc();
    let Some(sm) = sd.sd_meta.get() else {
        return;
    };
    // SAFETY: the discipline's metadata allocation, valid until `sr_discipline_free`.
    let sm: &SrMetadata = unsafe { sm.as_ref() };
    let cl = &sd.sd_vol.sv_chunk_list;
    let mut max_chunk_sz: i64 = 0;
    let mut min_chunk_sz: i64 = 0;
    let mut secsize = DEV_BSIZE as u32;

    // DNPRINTF(SR_D_META, "sr_meta_init")

    // Initialise volume metadata.
    let ssdi = sm.ssdi();
    ssdi.ssd_magic.set(SR_MAGIC);
    ssdi.ssd_version.set(SR_META_VERSION);
    ssdi.ssd_vol_flags.set(sd.sd_meta_flags.get());
    ssdi.ssd_volid.set(0);
    ssdi.ssd_chunk_no.set(no_chunk as u32);
    ssdi.ssd_level.set(level as u32);

    sm.ssd_data_blkno.set(SR_DATA_OFFSET as u32);
    sm.ssd_ondisk.set(0);

    let mut uuid = SrUuid::default();
    sr_uuid_generate(&mut uuid);
    ssdi.ssd_uuid.set(uuid);

    // Initialise chunk metadata and get min/max chunk sizes & secsize.
    for (cid, chunk) in cl.iter().enumerate() {
        let scm = &chunk.src_meta;
        scm.scmi().scm_size.set(chunk.src_size.get());
        scm.scmi().scm_chunk_id.set(cid as u32);
        scm.scm_status.set(BIOC_SDONLINE as u32);
        scm.scmi().scm_volid.set(0);
        sr_strlcpy_cell(&scm.scmi().scm_devname, &chunk.src_devname.get());
        scm.scmi().scm_uuid.set(uuid);
        // The C checksums `sizeof(scm->scm_checksum)` bytes from the start of the chunk
        // metadata (not the whole invariant part); kept for the on-disk format.
        scm.scm_checksum
            .set(sr_checksum(sc, &scm.cells()[..MD5_DIGEST_LENGTH]));

        let size = scm.scmi().scm_size.get();
        if min_chunk_sz == 0 {
            min_chunk_sz = size;
        }
        if chunk.src_secsize.get() > secsize {
            secsize = chunk.src_secsize.get();
        }
        min_chunk_sz = min_chunk_sz.min(size);
        max_chunk_sz = max_chunk_sz.max(size);
    }

    ssdi.ssd_secsize.set(secsize);

    // Equalize chunk sizes.
    for chunk in cl.iter() {
        chunk.src_meta.scmi().scm_coerced_size.set(min_chunk_sz);
    }

    sd.sd_vol.sv_chunk_minsz.set(min_chunk_sz);
    sd.sd_vol.sv_chunk_maxsz.set(max_chunk_sz);
}

/// `sr_meta_init_complete`: the volume's SCSI identity (`OPENBSD`, `SR <discipline>`, the
/// metadata version).
pub fn sr_meta_init_complete(sd: &SrDiscipline) {
    let sm = sd.sd_meta();

    // DNPRINTF(SR_D_META, "sr_meta_complete")

    // Complete initialisation of volume metadata.
    sr_strlcpy_cell(&sm.ssdi().ssd_vendor, b"OPENBSD");
    let mut product = [0u8; 16];
    let _ = snprintf(&mut product, format_args!("SR {}", sd.name()));
    sm.ssdi().ssd_product.set(product);
    let mut revision = [0u8; 4];
    let _ = snprintf(
        &mut revision,
        format_args!("{:03}", sm.ssdi().ssd_version.get()),
    );
    sm.ssdi().ssd_revision.set(revision);
}

/// `sr_meta_opt_handler`: the generic handler of the optional metadata a discipline did not
/// take: only `SR_OPT_BOOT` is known.
pub fn sr_meta_opt_handler(_sd: &SrDiscipline, om: &SrMetaOptHdr) {
    if om.som_type.get() != SR_OPT_BOOT {
        panic(format_args!("unknown optional metadata type"));
    }
}

/// `sr_meta_save_callback`: the `sd_meta_save_task`, at `splbio`.
pub fn sr_meta_save_callback(xsd: *mut c_void) {
    // SAFETY: `xsd` is the discipline `sr_discipline_init` set the task up with; a
    // discipline lives until `sr_discipline_free`, after its tasks.
    let sd: &'static SrDiscipline = unsafe { &*xsd.cast::<SrDiscipline>() };

    let s = splbio();

    if sr_meta_save(sd, SR_META_DIRTY).is_err() {
        printf(format_args!(
            "{}: save metadata failed\n",
            DEVNAME(sd.sd_sc())
        ));
    }

    sd.sd_must_flush.set(0);
    splx(s);
}

/// `sr_meta_save`: writes the volume's metadata (header, chunks, optional items) with
/// `flags` to every chunk that is not offline, bumping the on-disk version; a chunk whose
/// write fails is marked offline and the save restarts. Then syncs the discipline.
pub fn sr_meta_save(sd: &'static SrDiscipline, flags: u32) -> Result<(), Errno> {
    let sc = sd.sd_sc();

    // DNPRINTF(SR_D_META, "sr_meta_save %s")

    let Some(smp) = sd.sd_meta.get() else {
        printf(format_args!(
            "{}: no in memory copy of metadata\n",
            DEVNAME(sc)
        ));
        return Err(Errno::EIO);
    };
    // SAFETY: the discipline's metadata allocation, valid until `sr_discipline_free`.
    let sm: &SrMetadata = unsafe { smp.as_ref() };

    // meta scratchpad
    let s = smd(sd);
    let Some(mbuf) = SrMetaBuf::new(SR_META_BYTES, M_NOWAIT) else {
        printf(format_args!(
            "{}: could not allocate metadata scratch area\n",
            DEVNAME(sc)
        ));
        return Err(Errno::EIO);
    };
    let area = mbuf.cells();
    let m = mbuf.md();
    let chunk_no = sm.ssdi().ssd_chunk_no.get() as usize;

    // from here on out metadata is updated
    'restart: loop {
        sm.ssd_ondisk.set(sm.ssd_ondisk.get().wrapping_add(1));
        sm.ssd_meta_flags.set(flags);
        m.copy_from(sm);

        // Chunk metadata.
        for i in 0..chunk_no {
            let src = sd.sd_vol.sv_chunk(i);
            let Some(cm) = sr_meta_chunk_at(area, i) else {
                return Err(Errno::EIO);
            };
            cm.copy_from(&src.src_meta);
        }

        // Optional metadata.
        let mut off = size_of::<SrMetadata>() + chunk_no * size_of::<SrMetaChunk>();
        for omi in sd.sd_meta_opt.iter() {
            let som = omi.omi_som();
            let len = som.som_length.get() as usize;
            // DNPRINTF(SR_D_META, "saving optional metadata type %u with length %u")
            som.som_checksum.set([0; MD5_DIGEST_LENGTH]);
            let item = omi.som_cells();
            let Some(item) = item.get(..len) else {
                printf(format_args!(
                    "{}: invalid optional metadata length {}\n",
                    DEVNAME(sc),
                    len
                ));
                return Err(Errno::EIO);
            };
            som.som_checksum.set(sr_checksum(sc, item));
            let Some(dst) = area.get(off..off + len) else {
                printf(format_args!(
                    "{}: optional metadata does not fit\n",
                    DEVNAME(sc)
                ));
                return Err(Errno::EIO);
            };
            cells_copy(dst, item);
            off += len;
        }

        for i in 0..chunk_no {
            let src = sd.sd_vol.sv_chunk(i);

            // skip disks that are offline
            if src.src_meta.scm_status.get() == BIOC_SDOFFLINE as u32 {
                continue;
            }

            // calculate metadata checksum for correct chunk
            m.ssdi().ssd_chunk_id.set(i as u32);
            m.ssd_checksum.set(sr_checksum(sc, m.ssdi().cells()));

            // DNPRINTF(SR_D_META, "sr_meta_save %s: volid: %d chunkid: %d checksum: ")

            // translate and write to disk
            let write = s.smd_write.map_or(Err(Errno::EIO), |w| {
                w(
                    sd,
                    src.src_dev_mm.get(),
                    area,
                    ptr::null_mut(), /* XXX */
                )
            });
            if write.is_err() {
                printf(format_args!(
                    "{}: could not write metadata to {}\n",
                    DEVNAME(sc),
                    Name(src.src_devname.get())
                ));
                // restart the meta write
                src.src_meta.scm_status.set(BIOC_SDOFFLINE as u32);
                // XXX recalculate volume status
                continue 'restart;
            }
        }
        break;
    }

    // not all disciplines have sync
    if sd.sd_scsi_sync.get().is_some() {
        // The C's `struct sr_workunit wu` on the stack, zeroed.
        if let Some(wu) = sr_malloc::<SrWorkunit>(M_WAITOK) {
            // SAFETY: a zeroed work unit (`SrZeroed`), freed below after its only use.
            let wur: &'static SrWorkunit = unsafe { wu.as_ref() };
            wur.swu_flags.set(wur.swu_flags.get() | SR_WUF_FAKE);
            wur.swu_dis.set(ptr::from_ref(sd));
            let _ = sd.sd_scsi_sync(wur);
            sr_free(wu, size_of::<SrWorkunit>());
        }
    }
    Ok(())
}

/// `sr_meta_read`: reads and validates the metadata of every chunk that is not offline,
/// loads the first chunk's into `sd_meta` and its optional items, and each chunk's own
/// chunk metadata. Returns the number of chunks with metadata, or -1 on invalid metadata.
pub fn sr_meta_read(sd: &SrDiscipline) -> i32 {
    let sc = sd.sd_sc();
    let cl = &sd.sd_vol.sv_chunk_list;
    let mut no_disk = 0;
    let mut got_meta = false;

    // DNPRINTF(SR_D_META, "sr_meta_read")

    let Some(smb) = SrMetaBuf::new(SR_META_BYTES, M_WAITOK) else {
        return 0;
    };
    let sm = smb.md();
    let s = smd(sd);
    let fm = if sd.sd_meta_type.get() != SR_META_F_NATIVE {
        SrMetaBuf::new(s.smd_size, M_WAITOK)
    } else {
        None
    };
    let fmp = fm.as_ref().map_or(ptr::null_mut(), |f| {
        f.cells().as_ptr().cast_mut().cast::<c_void>()
    });

    let mut cp = 0usize;
    for ch_entry in cl.iter() {
        // skip disks that are offline
        if ch_entry.src_meta.scm_status.get() == BIOC_SDOFFLINE as u32 {
            // DNPRINTF(SR_D_META, "%s chunk marked offline, spoofing status")
            cp += 1; // adjust chunk pointer to match failure
            continue;
        }
        let read = s.smd_read.map_or(Err(Errno::EIO), |r| {
            r(sd, ch_entry.src_dev_mm.get(), smb.cells(), fmp)
        });
        if read.is_err() {
            // read and translate
            // XXX mark chunk offline, elsewhere!!
            ch_entry.src_meta.scm_status.set(BIOC_SDOFFLINE as u32);
            cp += 1; // adjust chunk pointer to match failure
            // DNPRINTF(SR_D_META, "sr_meta_read failed")
            continue;
        }

        if sm.ssdi().ssd_magic.get() != SR_MAGIC {
            // DNPRINTF(SR_D_META, "sr_meta_read !SR_MAGIC")
            continue;
        }

        // validate metadata
        if sr_meta_validate(sd, ch_entry.src_dev_mm.get(), sm, fmp).is_err() {
            // DNPRINTF(SR_D_META, "invalid metadata")
            return -1;
        }

        // assume first chunk contains metadata
        if !got_meta {
            sr_meta_opt_load(sc, smb.cells(), &sd.sd_meta_opt);
            sd.sd_meta().copy_from(sm);
            got_meta = true;
        }

        if let Some(c) = sr_meta_chunk_at(smb.cells(), cp) {
            ch_entry.src_meta.copy_from(c);
        }

        no_disk += 1;
        cp += 1;
    }

    // DNPRINTF(SR_D_META, "sr_meta_read found %d parts")
    no_disk
}

/// `sr_meta_opt_load`: loads the optional metadata items that follow the chunks in the
/// metadata area `sm` into `som`, converting the old fixed-length format; panics on a bad
/// checksum or an unknown old item, as the C does.
pub fn sr_meta_opt_load(sc: &SrSoftc, sm: &[Cell<u8>], som: &SrMetaOptHead) {
    let Some(md) = SrMetadata::view(sm) else {
        panic(format_args!("{}: invalid metadata area", DEVNAME(sc)));
    };
    let chunk_no = md.ssdi().ssd_chunk_no.get() as usize;
    let opt_no = md.ssdi().ssd_opt_no.get();

    // Process optional metadata.
    let mut off = size_of::<SrMetadata>() + size_of::<SrMetaChunk>() * chunk_no;
    for _ in 0..opt_no {
        let Some(omh) = sr_view::<SrMetaOptHdr>(sm, off) else {
            panic(format_args!(
                "{}: invalid optional metadata checksum",
                DEVNAME(sc)
            ));
        };

        if omh.som_length.get() == 0 {
            // Load old fixed length optional metadata.
            // DNPRINTF(SR_D_META, "old optional metadata of type %u")

            // Validate checksum.
            let old = sm.get(off..off + SR_OLD_META_OPT_SIZE);
            let ok = old.is_some_and(|old| {
                let checksum = sr_checksum(sc, &old[..SR_OLD_META_OPT_SIZE - MD5_DIGEST_LENGTH]);
                let mut stored = [0u8; MD5_DIGEST_LENGTH];
                cells_read(&mut stored, &old[SR_OLD_META_OPT_MD5..]);
                checksum == stored
            });
            if !ok {
                panic(format_args!(
                    "{}: invalid optional metadata checksum",
                    DEVNAME(sc)
                ));
            }

            // Determine correct length.
            let len = match omh.som_type.get() {
                SR_OPT_CRYPTO => size_of::<SrMetaCrypto>(),
                SR_OPT_BOOT => size_of::<SrMetaBoot>(),
                SR_OPT_KEYDISK => size_of::<SrMetaKeydisk>(),
                t => panic(format_args!("unknown old optional metadata type {}", t)),
            };
            omh.som_length.set(len as u32);

            let Some(omi) = SrMetaOptItem::alloc(len, M_WAITOK) else {
                panic(format_args!("sr_meta_opt_load: malloc"));
            };
            // SAFETY: a new item in no list; it lives until `sr_discipline_free` (or the
            // boot volume's teardown) unlinks and frees it.
            unsafe { som.insert_head(omi) };
            let hdr = size_of::<SrMetaOptHdr>();
            let src = &sm[off + SR_OLD_META_OPT_OFFSET..off + SR_OLD_META_OPT_OFFSET + len - hdr];
            cells_copy(&omi.som_cells()[hdr..], src);
            omi.omi_som().som_type.set(omh.som_type.get());
            omi.omi_som().som_length.set(len as u32);

            off += SR_OLD_META_OPT_SIZE;
        } else {
            // Load variable length optional metadata.
            // DNPRINTF(SR_D_META, "optional metadata of type %u, length %u")
            let len = omh.som_length.get() as usize;
            let Some(src) = sm.get(off..off + len) else {
                panic(format_args!(
                    "{}: invalid optional metadata checksum",
                    DEVNAME(sc)
                ));
            };
            let Some(omi) = SrMetaOptItem::alloc(len, M_WAITOK) else {
                panic(format_args!("sr_meta_opt_load: malloc"));
            };
            // SAFETY: as above.
            unsafe { som.insert_head(omi) };
            cells_copy(omi.som_cells(), src);

            // Validate checksum.
            let item = omi.omi_som();
            let checksum = item.som_checksum.get();
            item.som_checksum.set([0; MD5_DIGEST_LENGTH]);
            let sum = sr_checksum(sc, &omi.som_cells()[..len]);
            item.som_checksum.set(sum);
            if checksum != sum {
                panic(format_args!(
                    "{}: invalid optional metadata checksum",
                    DEVNAME(sc)
                ));
            }

            off += len;
        }
    }
}

/// `sr_meta_validate`: checks the (translated) metadata's magic and checksum and brings
/// versions 3 to 5 up to the current one.
pub fn sr_meta_validate(
    sd: &SrDiscipline,
    dev: Dev,
    sm: &SrMetadata,
    fm: *mut c_void,
) -> Result<(), Errno> {
    let sc = sd.sd_sc();
    let mut devname = [0u8; 32];

    // DNPRINTF(SR_D_META, "sr_meta_validate(%p)")

    sr_meta_getdevname(sc, dev, &mut devname);

    let s = smd(sd);
    if sd.sd_meta_type.get() != SR_META_F_NATIVE
        && s.smd_validate.is_some_and(|v| v(sd, sm, fm).is_err())
    {
        sr_error(sc, format_args!("invalid foreign metadata"));
        return Err(Errno::EIO);
    }

    // at this point all foreign metadata has been translated to the native format and will
    // be treated just like the native format

    let ssdi = sm.ssdi();
    if ssdi.ssd_magic.get() != SR_MAGIC {
        sr_error(sc, format_args!("not valid softraid metadata"));
        return Err(Errno::EIO);
    }

    // Verify metadata checksum.
    if sr_checksum(sc, ssdi.cells()) != sm.ssd_checksum.get() {
        sr_error(sc, format_args!("invalid metadata checksum"));
        return Err(Errno::EIO);
    }

    // Handle changes between versions.
    match ssdi.ssd_version.get() {
        3 => {
            // Version 3 - update metadata version and fix up data blkno value since this
            // did not exist in version 3.
            if sm.ssd_data_blkno.get() == 0 {
                sm.ssd_data_blkno.set(SR_META_V3_DATA_OFFSET as u32);
            }
            ssdi.ssd_secsize.set(DEV_BSIZE as u32);
        }
        4 => {
            // Version 4 - original metadata format did not store data blkno so fix this up
            // if necessary.
            if sm.ssd_data_blkno.get() == 0 {
                sm.ssd_data_blkno.set(SR_DATA_OFFSET as u32);
            }
            ssdi.ssd_secsize.set(DEV_BSIZE as u32);
        }
        5 => {
            // Version 5 - variable length optional metadata. Migration from earlier fixed
            // length optional metadata is handled in sr_meta_read().
            ssdi.ssd_secsize.set(DEV_BSIZE as u32);
        }
        SR_META_VERSION => {
            // Version 6 - store & report a sector size.
        }
        v => {
            sr_error(
                sc,
                format_args!(
                    "cannot read metadata version {} on {}, expected version {} or earlier",
                    v,
                    Str(&devname),
                    SR_META_VERSION
                ),
            );
            return Err(Errno::EIO);
        }
    }

    // Update version number and revision string.
    ssdi.ssd_version.set(SR_META_VERSION);
    let mut revision = [0u8; 4];
    let _ = snprintf(&mut revision, format_args!("{:03}", SR_META_VERSION));
    ssdi.ssd_revision.set(revision);

    // SR_DEBUG: warn if disk changed order (roaming device).

    // we have meta data on disk
    // DNPRINTF(SR_D_META, "sr_meta_validate valid metadata %s")

    Ok(())
}

/// `sr_meta_native_probe`: whether the chunk is a `RAID` partition big enough for the
/// metadata; records its DUID, usable size and sector size.
pub fn sr_meta_native_probe(_sc: &SrSoftc, ch_entry: &SrChunk) -> i32 {
    // DNPRINTF(SR_D_META, "sr_meta_native_probe(%s)")

    let part = diskpart(ch_entry.src_dev_mm.get()) as usize;
    let mut label = Disklabel::zeroed();

    let (Some(vn), Some(p)) = (ch_entry.src_vn.get(), curproc()) else {
        return SR_META_F_INVALID;
    };

    // get disklabel
    if VOP_IOCTL(vn, DIOCGDINFO, label.as_bytes_mut(), FREAD, NOCRED, p).is_err() {
        // DNPRINTF(SR_D_META, "%s can't obtain disklabel")
        return SR_META_F_INVALID;
    }
    ch_entry.src_duid.set(label.d_uid);

    // make sure the partition is of the right type
    let Some(pp) = label.d_partitions.get(part) else {
        return SR_META_F_INVALID;
    };
    if pp.p_fstype != FS_RAID {
        // DNPRINTF(SR_D_META, "%s partition not of type RAID (%d)")
        return SR_META_F_INVALID;
    }

    let mut size = dl_sectoblk(&label, dl_getpsize(pp));
    if size <= SR_DATA_OFFSET as u64 {
        // DNPRINTF(SR_D_META, "%s partition too small")
        return SR_META_F_INVALID;
    }
    size -= SR_DATA_OFFSET as u64;
    let Ok(size) = i64::try_from(size) else {
        // DNPRINTF(SR_D_META, "%s partition too large")
        return SR_META_F_INVALID;
    };
    ch_entry.src_size.set(size);
    ch_entry.src_secsize.set(label.d_secsize);

    // DNPRINTF(SR_D_META, "probe found %s size %lld")

    SR_META_F_NATIVE
}

/// `sr_meta_native_attach`: reads every chunk's native metadata, checks that they belong to
/// one volume, and marks chunks with an older on-disk version offline.
pub fn sr_meta_native_attach(sd: &'static SrDiscipline, force: i32) -> Result<(), Errno> {
    let sc = sd.sd_sc();
    let cl = &sd.sd_vol.sv_chunk_list;
    let mut version: u64 = 0;
    let (mut sr, mut not_sr, mut d) = (0, 0, 0);
    let mut expected: i64 = -1;
    let mut old_meta = 0;
    let mut uuid = SrUuid::default();

    // DNPRINTF(SR_D_META, "sr_meta_native_attach")

    let Some(mdb) = SrMetaBuf::new(SR_META_BYTES, M_NOWAIT) else {
        sr_error(sc, format_args!("not enough memory for metadata buffer"));
        return Err(Errno::EIO);
    };
    let md = mdb.md();

    for ch_entry in cl.iter() {
        if ch_entry.src_dev_mm.get() == NODEV {
            continue;
        }

        if sr_meta_native_read(sd, ch_entry.src_dev_mm.get(), mdb.cells(), ptr::null_mut()).is_err()
        {
            sr_error(sc, format_args!("could not read native metadata"));
            return Err(Errno::EIO);
        }

        if md.ssdi().ssd_magic.get() == SR_MAGIC {
            sr += 1;
            ch_entry
                .src_meta
                .scmi()
                .scm_chunk_id
                .set(md.ssdi().ssd_chunk_id.get());
            if d == 0 {
                uuid = md.ssdi().ssd_uuid.get();
                expected = i64::from(md.ssdi().ssd_chunk_no.get());
                version = md.ssd_ondisk.get();
                d += 1;
                continue;
            } else if md.ssdi().ssd_uuid.get() != uuid {
                sr_error(sc, format_args!("not part of the same volume"));
                return Err(Errno::EIO);
            }
            if md.ssd_ondisk.get() != version {
                old_meta += 1;
                version = md.ssd_ondisk.get().max(version);
            }
        } else {
            not_sr += 1;
        }
    }

    if sr != 0 && not_sr != 0 && force == 0 {
        sr_error(
            sc,
            format_args!("not all chunks are of the native metadata format"),
        );
        return Err(Errno::EIO);
    }

    // mixed metadata versions; mark bad disks offline
    if old_meta != 0 {
        for (d, ch_entry) in cl.iter().enumerate() {
            // XXX do we want to read this again?
            if ch_entry.src_dev_mm.get() == NODEV {
                panic(format_args!("src_dev_mm == NODEV"));
            }
            if sr_meta_native_read(sd, ch_entry.src_dev_mm.get(), mdb.cells(), ptr::null_mut())
                .is_err()
            {
                sr_warn(sc, format_args!("could not read native metadata"));
            }
            if md.ssd_ondisk.get() != version {
                sd.sd_vol
                    .sv_chunk(d)
                    .src_meta
                    .scm_status
                    .set(BIOC_SDOFFLINE as u32);
            }
        }
    }

    if expected != i64::from(sr) && force == 0 && expected != -1 {
        // DNPRINTF(SR_D_META, "not all chunks were provided, trying anyway")
    }

    Ok(())
}

/// `sr_meta_native_read`.
pub fn sr_meta_native_read(
    sd: &SrDiscipline,
    dev: Dev,
    md: &[Cell<u8>],
    _fm: *mut c_void,
) -> Result<(), Errno> {
    // DNPRINTF(SR_D_META, "sr_meta_native_read(0x%x, %p)")
    sr_meta_rw(sd, dev, md, B_READ)
}

/// `sr_meta_native_write`.
pub fn sr_meta_native_write(
    sd: &SrDiscipline,
    dev: Dev,
    md: &[Cell<u8>],
    _fm: *mut c_void,
) -> Result<(), Errno> {
    // DNPRINTF(SR_D_META, "sr_meta_native_write(0x%x, %p)")
    sr_meta_rw(sd, dev, md, B_WRITE)
}

/// `sr_hotplug_register`: calls `func` on disk attach and detach while `sd` is ready
/// (`sr_disk_attach`); a function is registered once.
pub fn sr_hotplug_register(sd: &'static SrDiscipline, func: SrHotplugFn) {
    // DNPRINTF(SR_D_MISC, "sr_hotplug_register: %p")

    // make sure we aren't on the list yet
    let list = &SR_HOTPLUG_CALLBACKS.0;
    if list.iter().any(|mhe| {
        mhe.sh_hotplug
            .get()
            .is_some_and(|f| ptr::fn_addr_eq(f, func))
    }) {
        return;
    }

    let Some(mhe) = sr_malloc::<SrHotplugList>(M_WAITOK) else {
        return;
    };
    // SAFETY: a zeroed entry (`SrZeroed`), freed only by `sr_hotplug_unregister`.
    let mhe: &'static SrHotplugList = unsafe { mhe.as_ref() };
    mhe.sh_hotplug.set(Some(func));
    mhe.sh_sd.set(Some(sd));
    // SAFETY: a new entry in no list; `'static` until unregistered.
    unsafe { list.insert_head(mhe) };
}

/// `sr_hotplug_unregister`.
pub fn sr_hotplug_unregister(_sd: &SrDiscipline, func: SrHotplugFn) {
    // DNPRINTF(SR_D_MISC, "sr_hotplug_unregister: %s %p")

    // make sure we are on the list yet
    let list = &SR_HOTPLUG_CALLBACKS.0;
    let found = list.iter().find(|mhe| {
        mhe.sh_hotplug
            .get()
            .is_some_and(|f| ptr::fn_addr_eq(f, func))
    });
    if let Some(mhe) = found {
        // SAFETY: `mhe` is on the list (found above); nothing else references it once it is
        // unlinked.
        unsafe { list.remove(mhe) };
        sr_free(NonNull::from(mhe), size_of::<SrHotplugList>());
    }
}

/// `sr_disk_attach`: `softraid_disk_attach`, called by `disk_attach`/`disk_detach`.
pub fn sr_disk_attach(diskp: &Disk, action: i32) {
    for mhe in SR_HOTPLUG_CALLBACKS.0.iter() {
        if let (Some(sd), Some(f)) = (mhe.sh_sd.get(), mhe.sh_hotplug.get())
            && sd.sd_ready.get() != 0
        {
            f(sd, diskp, action);
        }
    }
}

/// The softc's `bio_status`, which `sc_lock` (write) protects.
fn sr_status(sc: &SrSoftc, print: bool, msg_type: i32, args: core::fmt::Arguments<'_>) {
    rw_assert_wrlock(&sc.sc_lock);

    // SAFETY: the caller holds `sc_lock` for writing (asserted under `diagnostic`), which
    // serialises every use of `sc_status` (the bio handler and these three functions).
    let bs = unsafe { &mut *sc.sc_status.get() };
    bio_status(bs, print, msg_type, args);
}

/// `sr_info`: an informational message for bioctl(8).
pub fn sr_info(sc: &SrSoftc, args: core::fmt::Arguments<'_>) {
    sr_status(sc, false, BIO_MSG_INFO, args);
}

/// `sr_warn`: a warning for bioctl(8), also printed.
pub fn sr_warn(sc: &SrSoftc, args: core::fmt::Arguments<'_>) {
    sr_status(sc, true, BIO_MSG_WARN, args);
}

/// `sr_error`: an error for bioctl(8), also printed.
pub fn sr_error(sc: &SrSoftc, args: core::fmt::Arguments<'_>) {
    sr_status(sc, true, BIO_MSG_ERROR, args);
}

/// `sr_ccb_alloc`: the discipline's `sd_max_wu * sd_max_ccb_per_wu` ccbs, all free.
/// Fails (the C's 1) when they exist already.
pub fn sr_ccb_alloc(sd: &'static SrDiscipline) -> Result<(), Errno> {
    // DNPRINTF(SR_D_CCB, "sr_ccb_alloc")

    if sd.sd_ccb.get().is_some() {
        return Err(Errno::EIO);
    }

    let n = sd.sd_max_wu.get() as usize * sd.sd_max_ccb_per_wu.get() as usize;
    let Some(mem) = mallocarray(n.max(1), size_of::<SrCcb>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
        return Err(Errno::ENOMEM);
    };
    let base = mem.cast::<SrCcb>();
    sd.sd_ccb.set(Some(base));
    sd.sd_nccb.set(n);
    sd.sd_ccb_freeq.init();
    for i in 0..n {
        // SAFETY: slot `i` of an array of `n` ccbs (`mallocarray`, aligned to its
        // power-of-two size), written once before use.
        let ccb: &'static SrCcb = unsafe {
            let p = base.as_ptr().add(i);
            p.write(SrCcb::new());
            &*p
        };
        ccb.ccb_dis.set(ptr::from_ref(sd));
        sr_ccb_put(ccb);
    }

    // DNPRINTF(SR_D_CCB, "sr_ccb_alloc ccb: %d")

    Ok(())
}

/// `sr_ccb_free`.
pub fn sr_ccb_free(sd: &SrDiscipline) {
    // DNPRINTF(SR_D_CCB, "sr_ccb_free %p")

    while let Some(ccb) = sd.sd_ccb_freeq.first() {
        // SAFETY: `ccb` is the queue's first element.
        unsafe { sd.sd_ccb_freeq.remove(ccb) };
    }

    if let Some(p) = sd.sd_ccb.take() {
        let n = sd.sd_nccb.replace(0);
        free(p.cast::<u8>(), M_DEVBUF, n.max(1) * size_of::<SrCcb>());
    }
}

/// `sr_ccb_get`: a free ccb, in progress; `None` when there is none.
pub fn sr_ccb_get(sd: &'static SrDiscipline) -> Option<&'static SrCcb> {
    let s = splbio();

    let ccb = sd.sd_ccb_freeq.first();
    if let Some(ccb) = ccb {
        // SAFETY: the queue's first element, at `splbio`.
        unsafe { sd.sd_ccb_freeq.remove(ccb) };
        ccb.ccb_state.set(SR_CCB_INPROGRESS);
    }

    splx(s);

    // DNPRINTF(SR_D_CCB, "sr_ccb_get: %p")

    ccb
}

/// `sr_ccb_put`: gives a ccb back to its discipline's free queue.
pub fn sr_ccb_put(ccb: &'static SrCcb) {
    let sd = ccb.dis();

    // DNPRINTF(SR_D_CCB, "sr_ccb_put: %p")

    let s = splbio();

    ccb.ccb_wu.set(None);
    ccb.ccb_state.set(SR_CCB_FREE);
    ccb.ccb_target.set(-1);
    ccb.ccb_opaque.set(ptr::null_mut());

    // SAFETY: a ccb being put is on no queue (taken by `sr_ccb_get` and released from its
    // work unit, or new); ccbs live until `sr_ccb_free`; at `splbio`.
    unsafe { sd.sd_ccb_freeq.insert_tail(ccb) };

    splx(s);
}

/// `sr_ccb_rw`: a ccb for `len` bytes at `data` to or from (`SCSI_DATA_IN` in `xsflags`)
/// block `blkno` of the volume's data area on chunk `chunk`, completed by `sd_scsi_intr`;
/// `None` when no ccb is free.
///
/// # Safety
///
/// `data` is valid for reads and writes of `len` bytes, and reserved for this I/O, until the
/// ccb completes (`sd_scsi_intr` has run) and is put back.
pub unsafe fn sr_ccb_rw(
    sd: &'static SrDiscipline,
    chunk: usize,
    blkno: Daddr,
    len: i64,
    data: *mut u8,
    xsflags: i32,
    ccbflags: i32,
) -> Option<&'static SrCcb> {
    let sc = sd.sd_vol.sv_chunk(chunk);

    let ccb = sr_ccb_get(sd)?;

    ccb.ccb_flags.set(ccbflags);
    ccb.ccb_target.set(chunk as i32);

    let b = &ccb.ccb_buf;
    b.b_flags.set(B_PHYS | B_CALL);
    if xsflags & SCSI_DATA_IN != 0 {
        b.set(B_READ);
    } else {
        b.set(B_WRITE);
    }

    b.b_blkno
        .set(blkno + Daddr::from(sd.sd_meta().ssd_data_blkno.get()));
    b.b_bcount.set(len);
    b.b_bufsize.set(len);
    b.b_resid.set(usize::try_from(len).unwrap_or(0));
    b.b_data.set(data);
    b.b_error.set(None);
    b.b_iodone.set(sd.sd_scsi_intr.get());
    b.b_proc.set(curproc().map_or(ptr::null(), ptr::from_ref));
    b.b_dev.set(sc.src_dev_mm.get());
    b.b_vp.set(sc.src_vn.get());
    b.b_bq.set(None);

    if !b.isset(B_READ)
        && let Some(vp) = b.b_vp.get()
    {
        let s = splbio();
        vp.v_numoutput.set(vp.v_numoutput.get() + 1);
        splx(s);
    }

    // DNPRINTF(SR_D_DIS, "%s %s ccb b_bcount %ld b_blkno %lld b_flags 0x%0lx b_data %p")

    Some(ccb)
}

/// `sr_ccb_done`: accounts a finished ccb to its work unit; an I/O error takes the chunk
/// offline on a redundant volume. At `splbio`.
pub fn sr_ccb_done(ccb: &'static SrCcb) {
    let wu = ccb.wu();
    let sd = wu.dis();
    let sc = sd.sd_sc();

    // DNPRINTF(SR_D_INTR, "%s %s %s ccb done ...")

    splassert(IPL_BIO, "sr_ccb_done");

    if ccb.ccb_target.get() == -1 {
        panic(format_args!(
            "{}: invalid target on wu: {:p}",
            DEVNAME(sc),
            wu
        ));
    }

    let b = &ccb.ccb_buf;
    if b.isset(B_ERROR) {
        // DNPRINTF(SR_D_INTR, "i/o error on block %lld target %d")
        if sd.sd_capabilities.get() & SR_CAP_REDUNDANT != 0 {
            sd.sd_set_chunk_state(ccb.ccb_target.get() as usize, BIOC_SDOFFLINE);
        } else {
            printf(format_args!(
                "{}: {}: i/o error {} @ {} block {}\n",
                DEVNAME(sc),
                Name(sd.sd_meta().ssd_devname.get()),
                b.b_error.get().map_or(0, |e| e as i32),
                sd.name(),
                b.b_blkno.get()
            ));
        }
        ccb.ccb_state.set(SR_CCB_FAILED);
        wu.swu_ios_failed.set(wu.swu_ios_failed.get() + 1);
    } else {
        ccb.ccb_state.set(SR_CCB_OK);
        wu.swu_ios_succeeded.set(wu.swu_ios_succeeded.get() + 1);
    }

    wu.swu_ios_complete.set(wu.swu_ios_complete.get() + 1);
}

/// `sr_wu_alloc`: the discipline's `sd_max_wu` work units (`sd_wu_size` bytes each), all
/// free, and its queues.
pub fn sr_wu_alloc(sd: &'static SrDiscipline) -> Result<(), Errno> {
    // DNPRINTF(SR_D_WU, "sr_wu_alloc %p %d")

    let no_wu = sd.sd_max_wu.get();
    sd.sd_wu_pending.set(no_wu as i32);

    mtx_init(&sd.sd_wu_mtx, IPL_BIO);
    sd.sd_wu.init();
    sd.sd_wu_freeq.init();
    sd.sd_wu_pendq.init();
    sd.sd_wu_defq.init();

    for _ in 0..no_wu {
        let Some(p) = sr_malloc_size::<SrWorkunit>(sd.sd_wu_size(), M_WAITOK) else {
            return Err(Errno::ENOMEM);
        };
        // SAFETY: `sd_wu_size` zeroed bytes, at least a work unit, valid as zero
        // (`SrZeroed`, or the discipline's `SrWorkunitExt`); freed by `sr_wu_free`.
        let wu: &'static SrWorkunit = unsafe { p.as_ref() };
        // SAFETY: a new work unit in no list; it lives until `sr_wu_free`.
        unsafe { sd.sd_wu.insert_tail(wu) };
        wu.swu_ccb.init();
        wu.swu_dis.set(ptr::from_ref(sd));
        task_set(&wu.swu_task, sr_wu_done_callback, p.as_ptr().cast());
        wu_put(sd, wu);
    }

    Ok(())
}

/// `sr_wu_free`: empties the queues and frees every work unit.
pub fn sr_wu_free(sd: &SrDiscipline) {
    // DNPRINTF(SR_D_WU, "sr_wu_free %p")

    for q in [&sd.sd_wu_freeq, &sd.sd_wu_pendq, &sd.sd_wu_defq] {
        while let Some(wu) = q.first() {
            // SAFETY: the queue's first element.
            unsafe { q.remove(wu) };
        }
    }

    while let Some(wu) = sd.sd_wu.first() {
        // SAFETY: the list's first element; nothing references the unit once it is
        // unlinked from every list.
        unsafe { sd.sd_wu.remove(wu) };
        sr_free(NonNull::from(wu), sd.sd_wu_size());
    }
}

/// `sr_wu_get` without the `void *`: a free work unit, or `None`.
fn wu_get(sd: &SrDiscipline) -> Option<&'static SrWorkunit> {
    mtx_enter(&sd.sd_wu_mtx);
    let wu = sd.sd_wu_freeq.first().map(|wu| {
        // SAFETY: the queue's first element, under `sd_wu_mtx`.
        unsafe { sd.sd_wu_freeq.remove(wu) };
        sd.sd_wu_pending.set(sd.sd_wu_pending.get() + 1);
        // SAFETY: work units live (in `sd_wu`) until `sr_wu_free`, which runs only once the
        // volume's I/O has stopped.
        unsafe { &*ptr::from_ref(wu) }
    });
    mtx_leave(&sd.sd_wu_mtx);

    // DNPRINTF(SR_D_WU, "sr_wu_get: %p")

    wu
}

/// `sr_wu_put` without the `void *`s.
fn wu_put(sd: &SrDiscipline, wu: &'static SrWorkunit) {
    // DNPRINTF(SR_D_WU, "sr_wu_put: %p")

    sr_wu_release_ccbs(wu);
    sr_wu_init(sd, wu);

    mtx_enter(&sd.sd_wu_mtx);
    // SAFETY: a work unit being put is on no processing queue (taken by `sr_wu_get`, or
    // new); it lives until `sr_wu_free`; under `sd_wu_mtx`.
    unsafe { sd.sd_wu_freeq.insert_tail(wu) };
    sd.sd_wu_pending.set(sd.sd_wu_pending.get() - 1);
    mtx_leave(&sd.sd_wu_mtx);
}

/// `sr_wu_get`: the `io_get` of the discipline's `scsi_iopool`.
///
/// # Safety
///
/// `xsd` is the `&'static SrDiscipline` the pool was initialised with.
pub unsafe fn sr_wu_get(xsd: *mut c_void) -> Option<ScsiIo> {
    // SAFETY: the caller's contract.
    let sd = unsafe { &*xsd.cast::<SrDiscipline>() };
    wu_get(sd).map(|wu| NonNull::from(wu).cast())
}

/// `sr_wu_put`: the `io_put` of the discipline's `scsi_iopool`.
///
/// # Safety
///
/// `xsd` is the `&'static SrDiscipline` the pool was initialised with, and `xwu` a work unit
/// `sr_wu_get` handed out for it.
pub unsafe fn sr_wu_put(xsd: *mut c_void, xwu: ScsiIo) {
    // SAFETY: the caller's contract.
    let (sd, wu) = unsafe {
        (
            &*xsd.cast::<SrDiscipline>(),
            &*xwu.as_ptr().cast::<SrWorkunit>(),
        )
    };
    wu_put(sd, wu);
}

/// `sr_wu_init`: resets a work unit; panics on one whose ccbs are being started.
pub fn sr_wu_init(sd: &SrDiscipline, wu: &SrWorkunit) {
    let s = splbio();
    if wu.swu_cb_active.get() == 1 {
        panic(format_args!(
            "{}: sr_wu_init got active wu",
            DEVNAME(sd.sd_sc())
        ));
    }
    splx(s);

    wu.swu_xs.set(None);
    wu.swu_state.set(SR_WU_FREE);
    wu.swu_flags.set(0);
    wu.swu_blk_start.set(0);
    wu.swu_blk_end.set(0);
    wu.swu_collider.set(None);
}

/// `sr_wu_enqueue_ccb`: adds a ccb to the work unit's I/Os.
pub fn sr_wu_enqueue_ccb(wu: &'static SrWorkunit, ccb: &'static SrCcb) {
    let sd = wu.dis();

    let s = splbio();
    if wu.swu_cb_active.get() == 1 {
        panic(format_args!(
            "{}: sr_wu_enqueue_ccb got active wu",
            DEVNAME(sd.sd_sc())
        ));
    }
    ccb.ccb_wu.set(Some(wu));
    wu.swu_io_count.set(wu.swu_io_count.get() + 1);
    // SAFETY: a ccb from `sr_ccb_get` is on no queue; ccbs live until `sr_ccb_free`; at
    // `splbio`.
    unsafe { wu.swu_ccb.insert_tail(ccb) };
    splx(s);
}

/// `sr_wu_release_ccbs`: returns all ccbs that are associated with this workunit.
pub fn sr_wu_release_ccbs(wu: &'static SrWorkunit) {
    while let Some(ccb) = wu.swu_ccb.first() {
        // SAFETY: the queue's first element.
        unsafe { wu.swu_ccb.remove(ccb) };
        sr_ccb_put(ccb);
    }

    wu.swu_io_count.set(0);
    wu.swu_ios_complete.set(0);
    wu.swu_ios_failed.set(0);
    wu.swu_ios_succeeded.set(0);
}

/// `sr_wu_done`: once every I/O of the work unit is complete, queues
/// `sr_wu_done_callback` on the discipline's task queue.
pub fn sr_wu_done(wu: &'static SrWorkunit) {
    let sd = wu.dis();

    // DNPRINTF(SR_D_INTR, "sr_wu_done count %d completed %d failed %d")

    if wu.swu_ios_complete.get() < wu.swu_io_count.get() {
        return;
    }

    let Some(tq) = sd.sd_taskq.get() else {
        panic(format_args!(
            "{}: discipline without taskq",
            DEVNAME(sd.sd_sc())
        ));
    };
    let _ = task_add(tq, &wu.swu_task);
}

/// `sr_wu_done_callback`: completes a work unit whose I/O is done: sets the transfer's
/// error, lets the discipline restart it (`sd_scsi_wu_done`), takes it off the pending
/// queue, starts the work unit that collided with it, and completes the transfer.
pub fn sr_wu_done_callback(xwu: *mut c_void) {
    // SAFETY: `xwu` is the work unit `sr_wu_alloc` set this task up with; it lives until
    // `sr_wu_free`.
    let wu: &'static SrWorkunit = unsafe { &*xwu.cast::<SrWorkunit>() };
    let sd = wu.dis();
    let xs = wu.swu_xs.get();

    // The SR_WUF_DISCIPLINE or SR_WUF_REBUILD flag must be set if the work unit is not
    // associated with a scsi_xfer.
    kassert!(xs.is_some() || wu.swu_flags.get() & (SR_WUF_DISCIPLINE | SR_WUF_REBUILD) != 0);

    let s = splbio();

    'done: {
        if let Some(xs) = xs {
            if wu.swu_ios_failed.get() != 0 {
                xs.error.set(XS_DRIVER_STUFFUP);
            } else {
                xs.error.set(XS_NOERROR);
            }
        }

        if sd.sd_scsi_wu_done.get().is_some() && sd.sd_scsi_wu_done(wu) == SR_WU_RESTART {
            break 'done;
        }

        // Remove work unit from pending queue.
        if !sd.sd_wu_pendq.iter().any(|wup| ptr::eq(wup, wu)) {
            panic(format_args!(
                "{}: wu {:p} not on pending queue",
                DEVNAME(sd.sd_sc()),
                wu
            ));
        }
        // SAFETY: on the pending queue (checked above); at `splbio`.
        unsafe { sd.sd_wu_pendq.remove(wu) };

        if let Some(collider) = wu.swu_collider.get() {
            if wu.swu_ios_failed.get() != 0 {
                sr_raid_recreate_wu(collider);
            }

            // XXX Should the collider be failed if this xs failed?
            sr_raid_startwu(collider);
        }

        // If a discipline provides its own sd_scsi_done function, then it is responsible
        // for calling sr_scsi_done() once I/O is complete.
        if wu.swu_flags.get() & SR_WUF_REBUILD != 0 {
            wu.swu_flags.set(wu.swu_flags.get() | SR_WUF_REBUILDIOCOMP);
        }
        if wu.swu_flags.get() & SR_WUF_WAKEUP != 0 {
            wakeup(ptr::from_ref(wu));
        }
        if sd.sd_scsi_done.get().is_some() {
            sd.sd_scsi_done(wu);
        } else if wu.swu_flags.get() & SR_WUF_DISCIPLINE != 0 {
            sr_scsi_wu_put(sd, wu);
        } else if wu.swu_flags.get() & SR_WUF_REBUILD == 0
            && let Some(xs) = xs
        {
            sr_scsi_done(sd, xs);
        }
    }

    splx(s);
}

/// `sr_scsi_wu_get`: a work unit from the volume's `scsi_iopool` (`flags`: `SCSI_NOSLEEP`
/// or 0); `None` when none is free and the caller may not sleep.
pub fn sr_scsi_wu_get(sd: &'static SrDiscipline, flags: i32) -> Option<&'static SrWorkunit> {
    let io = scsi_io_get(&sd.sd_iopool, flags)?;
    // SAFETY: the pool's `io_get` is `sr_wu_get` with this discipline as cookie (set up with
    // the pool), which hands out the discipline's work units; they live until `sr_wu_free`.
    Some(unsafe { &*io.as_ptr().cast::<SrWorkunit>() })
}

/// `sr_scsi_wu_put`: gives a work unit back to the volume's `scsi_iopool`.
pub fn sr_scsi_wu_put(sd: &SrDiscipline, wu: &'static SrWorkunit) {
    scsi_io_put(&sd.sd_iopool, NonNull::from(wu).cast());

    if sd.sd_sync.get() != 0 && sd.sd_wu_pending.get() == 0 {
        wakeup(ptr::from_ref(sd));
    }
}

/// `sr_scsi_done`: completes a transfer of the volume.
pub fn sr_scsi_done(sd: &SrDiscipline, xs: &'static ScsiXfer) {
    // DNPRINTF(SR_D_DIS, "sr_scsi_done: xs %p")

    if xs.error.get() == XS_NOERROR {
        xs.resid.set(0);
    }

    scsi_done(xs);

    if sd.sd_sync.get() != 0 && sd.sd_wu_pending.get() == 0 {
        wakeup(ptr::from_ref(sd));
    }
}

/// `sr_chunk_in_use`: the status of the chunk on `dev` in a volume or among the hotspares,
/// or `BIOC_SDINVALID`.
pub fn sr_chunk_in_use(sc: &SrSoftc, dev: Dev) -> i32 {
    // DNPRINTF(SR_D_MISC, "sr_chunk_in_use(%d)")

    if dev == NODEV {
        return BIOC_SDINVALID;
    }

    // See if chunk is already in use.
    for sd in sc.sc_dis_list.iter() {
        for i in 0..sd.sd_meta().ssdi().ssd_chunk_no.get() as usize {
            let chunk = sd.sd_vol.sv_chunk(i);
            if chunk.src_dev_mm.get() == dev {
                return chunk.src_meta.scm_status.get() as i32;
            }
        }
    }

    // Check hotspares list.
    for chunk in sc.sc_hotspare_list.iter() {
        if chunk.src_dev_mm.get() == dev {
            return chunk.src_meta.scm_status.get() as i32;
        }
    }

    BIOC_SDINVALID
}

/// `sr_hotspare_rebuild_callback`: the `sd_hotspare_rebuild_task`.
pub fn sr_hotspare_rebuild_callback(xsd: *mut c_void) {
    // SAFETY: `xsd` is the discipline `sr_discipline_init` set the task up with.
    let sd: &'static SrDiscipline = unsafe { &*xsd.cast::<SrDiscipline>() };
    sr_hotspare_rebuild(sd);
}

/// `sr_hotspare_rebuild`: looks for a hotspare to rebuild a degraded volume onto.
pub fn sr_hotspare_rebuild(sd: &'static SrDiscipline) {
    let _ = sd;
    let _ = unported!("sr_hotspare_rebuild (softraid.c, phase 2 of the port)");
}

/// `sr_rebuild_percent`: how far the rebuild has got, in percent.
pub fn sr_rebuild_percent(sd: &SrDiscipline) -> i32 {
    let sz = sd.sd_meta().ssdi().ssd_size.get();
    let rb = sd.sd_meta().ssd_rebuild.get();

    if rb > 0 && sz > 0 {
        return (100 - ((sz * 100 - rb * 100) / sz) - 1) as i32;
    }

    0
}

/// `sr_discipline_init`: installs the default hooks and the discipline of RAID `level`
/// (`0`, `1`, `5`, `6`, `'C'`, `0x1C`, `'c'`); fails for an unknown level.
pub fn sr_discipline_init(sd: &'static SrDiscipline, level: i32) -> Result<(), Errno> {
    // Initialise discipline function pointers with defaults.
    sd.sd_alloc_resources.set(Some(sr_alloc_resources));
    sd.sd_assemble.set(None);
    sd.sd_create.set(None);
    sd.sd_free_resources.set(Some(sr_free_resources));
    sd.sd_ioctl_handler.set(None);
    sd.sd_openings.set(None);
    sd.sd_meta_opt_handler.set(None);
    sd.sd_rebuild.set(Some(sr_rebuild));
    sd.sd_scsi_inquiry.set(Some(sr_raid_inquiry));
    sd.sd_scsi_read_cap.set(Some(sr_raid_read_cap));
    sd.sd_scsi_tur.set(Some(sr_raid_tur));
    sd.sd_scsi_req_sense.set(Some(sr_raid_request_sense));
    sd.sd_scsi_start_stop.set(Some(sr_raid_start_stop));
    sd.sd_scsi_sync.set(Some(sr_raid_sync));
    sd.sd_scsi_rw.set(None);
    sd.sd_scsi_intr.set(Some(sr_raid_intr));
    sd.sd_scsi_wu_done.set(None);
    sd.sd_scsi_done.set(None);
    sd.sd_set_chunk_state.set(Some(sr_set_chunk_state));
    sd.sd_set_vol_state.set(Some(sr_set_vol_state));
    sd.sd_start_discipline.set(None);

    let xsd: *mut c_void = ptr::from_ref(sd).cast_mut().cast();
    task_set(&sd.sd_meta_save_task, sr_meta_save_callback, xsd);
    task_set(
        &sd.sd_hotspare_rebuild_task,
        sr_hotspare_rebuild_callback,
        xsd,
    );

    sd.set_wu_type_default();
    match level {
        0 => Err(unported!("sr_raid0_discipline_init (softraid_raid0.c)")),
        1 => Err(unported!("sr_raid1_discipline_init (softraid_raid1.c)")),
        5 => Err(unported!("sr_raid5_discipline_init (softraid_raid5.c)")),
        6 => Err(unported!("sr_raid6_discipline_init (softraid_raid6.c)")),
        // CRYPTO
        0x43 /* 'C' */ => Err(unported!("sr_crypto_discipline_init (softraid_crypto.c)")),
        0x1C => Err(unported!("sr_raid1c_discipline_init (softraid_raid1c.c)")),
        0x63 /* 'c' */ => Err(unported!("sr_concat_discipline_init (softraid_concat.c)")),
        _ => Err(Errno::EIO),
    }
}

/// `sr_raid_inquiry`: the default INQUIRY: a direct-access SCSI-2 disk with the volume's
/// vendor, product and revision.
pub fn sr_raid_inquiry(wu: &'static SrWorkunit) -> Result<(), Errno> {
    let sd = wu.dis();
    let xs = wu.xs();

    // DNPRINTF(SR_D_DIS, "sr_raid_inquiry")

    if xs.cmdlen.get() != size_of::<ScsiInquiry>() as i32 {
        return Err(Errno::EINVAL);
    }
    let cdb = xs.cmd_as::<ScsiInquiry>();

    if cdb.flags & SI_EVPD != 0 {
        return Err(Errno::EOPNOTSUPP);
    }

    let ssdi = sd.sd_meta().ssdi();
    let mut inq = ScsiInquiryData::zeroed();
    inq.device = T_DIRECT;
    inq.dev_qual2 = 0;
    inq.version = SCSI_REV_2;
    inq.response_format = SID_SCSI2_RESPONSE;
    inq.additional_length = SID_SCSI2_ALEN as u8;
    inq.flags |= SID_CmdQue;
    let _ = strlcpy(&mut inq.vendor, &ssdi.ssd_vendor.get());
    let _ = strlcpy(&mut inq.product, &ssdi.ssd_product.get());
    let _ = strlcpy(&mut inq.revision, &ssdi.ssd_revision.get());
    scsi_copy_internal_data(xs, inq.as_bytes());

    Ok(())
}

/// `sr_raid_read_cap`: the default READ CAPACITY (10 and 16), from the volume size and
/// sector size.
pub fn sr_raid_read_cap(wu: &'static SrWorkunit) -> Result<(), Errno> {
    let sd = wu.dis();
    let xs = wu.xs();

    // DNPRINTF(SR_D_DIS, "sr_raid_read_cap")

    let secsize = sd.sd_meta().ssdi().ssd_secsize.get();

    let size = sd.sd_meta().ssdi().ssd_size.get() as u64;
    let addr = ((size * DEV_BSIZE as u64) / u64::from(secsize)).wrapping_sub(1);
    let opcode = xs.cmd.get().opcode;
    if opcode == READ_CAPACITY {
        let mut rcd = ScsiReadCapData::zeroed();
        if addr > 0xffff_ffff {
            _lto4b(0xffff_ffff, &mut rcd.addr);
        } else {
            _lto4b(addr as u32, &mut rcd.addr);
        }
        _lto4b(secsize, &mut rcd.length);
        scsi_copy_internal_data(xs, rcd.as_bytes());
        Ok(())
    } else if opcode == READ_CAPACITY_16 {
        let mut rcd16 = ScsiReadCapData16::zeroed();
        _lto8b(addr, &mut rcd16.addr);
        _lto4b(secsize, &mut rcd16.length);
        scsi_copy_internal_data(xs, rcd16.as_bytes());
        Ok(())
    } else {
        Err(Errno::EIO)
    }
}

/// Sets the discipline's sense data (`sd_scsi_sense`) to `key` with the ASC/ASCQ.
fn sr_set_sense(sd: &SrDiscipline, error_code: u8, key: u8, asc: u8, ascq: u8) {
    let mut s = sd.sd_scsi_sense.get();
    s.error_code = error_code;
    s.flags = key;
    s.add_sense_code = asc;
    s.add_sense_code_qual = ascq;
    s.extra_len = 4;
    sd.sd_scsi_sense.set(s);
}

/// `sr_raid_tur`: the default TEST UNIT READY: not ready while the volume is offline, a
/// hardware error while it is invalid.
pub fn sr_raid_tur(wu: &'static SrWorkunit) -> Result<(), Errno> {
    let sd = wu.dis();

    // DNPRINTF(SR_D_DIS, "sr_raid_tur")

    if sd.sd_vol_status.get() == BIOC_SVOFFLINE {
        sr_set_sense(sd, SSD_ERRCODE_CURRENT, SKEY_NOT_READY, 0x04, 0x11);
        return Err(Errno::EIO);
    } else if sd.sd_vol_status.get() == BIOC_SVINVALID {
        sr_set_sense(sd, SSD_ERRCODE_CURRENT, SKEY_HARDWARE_ERROR, 0x05, 0x00);
        return Err(Errno::EIO);
    }

    Ok(())
}

/// `sr_raid_request_sense`: the default REQUEST SENSE: the latest sense data, then cleared.
pub fn sr_raid_request_sense(wu: &'static SrWorkunit) -> Result<(), Errno> {
    let sd = wu.dis();
    let xs = wu.xs();

    // DNPRINTF(SR_D_DIS, "sr_raid_request_sense")

    // use latest sense data
    xs.sense.set(sd.sd_scsi_sense.get());

    // clear sense data
    sd.sd_scsi_sense.set(Default::default());

    Ok(())
}

/// `sr_raid_start_stop`: the default START STOP: do nothing! A softraid discipline should
/// always reflect correct status.
pub fn sr_raid_start_stop(wu: &'static SrWorkunit) -> Result<(), Errno> {
    // DNPRINTF(SR_D_DIS, "sr_raid_start_stop")
    // `if (!ss) return (1)`: `&xs->cmd` is never NULL.
    let _ = wu;
    Ok(())
}

/// `sr_raid_sync`: the default SYNCHRONIZE CACHE: waits (up to 15 seconds per wakeup) for
/// the volume's other work units to finish.
pub fn sr_raid_sync(wu: &'static SrWorkunit) -> Result<(), Errno> {
    let sd = wu.dis();
    let mut rv = Ok(());

    // DNPRINTF(SR_D_DIS, "sr_raid_sync")

    // when doing a fake sync don't count the wu
    let ios = if wu.swu_flags.get() & SR_WUF_FAKE != 0 {
        0
    } else {
        1
    };

    let s = splbio();
    sd.sd_sync.set(1);
    while sd.sd_wu_pending.get() > ios {
        let ret = tsleep_nsec(ptr::from_ref(sd), PRIBIO, "sr_sync", sec_to_nsec(15));
        if ret == Err(Errno::EWOULDBLOCK) {
            // DNPRINTF(SR_D_DIS, "sr_raid_sync timeout")
            rv = Err(Errno::EIO);
            break;
        }
    }
    sd.sd_sync.set(0);
    splx(s);

    wakeup(ptr::from_ref(&sd.sd_sync));

    rv
}

/// `sr_raid_intr`: the default `sd_scsi_intr`: a ccb's buffer is done.
pub fn sr_raid_intr(bp: &'static Buf) {
    // SAFETY: `sr_raid_intr` is only installed as `sd_scsi_intr`, which `sr_ccb_rw` makes
    // the `b_iodone` of a ccb's own buffer.
    let ccb = unsafe { sr_ccb_from_buf(bp) };
    let wu = ccb.wu();

    // DNPRINTF(SR_D_INTR, "%s %s %s intr bp %p xs %p")

    let s = splbio();
    sr_ccb_done(ccb);
    sr_wu_done(wu);
    splx(s);
}

/// `sr_schedule_wu`: starts a work unit, or defers it behind the pending one whose block
/// range it overlaps (its collider), which starts it when done.
pub fn sr_schedule_wu(wu: &'static SrWorkunit) {
    let sd = wu.dis();

    // DNPRINTF(SR_D_WU, "sr_schedule_wu: schedule wu %p state %i flags 0x%x")

    kassert!(wu.swu_io_count.get() > 0);

    let s = splbio();

    'queued: {
        // Construct the work unit, do not schedule it.
        if wu.swu_state.get() == SR_WU_CONSTRUCT {
            break 'queued;
        }

        // Deferred work unit being reconstructed, do not start.
        if wu.swu_state.get() == SR_WU_REQUEUE {
            break 'queued;
        }

        // Current work unit failed, restart.
        if wu.swu_state.get() != SR_WU_RESTART {
            if wu.swu_state.get() != SR_WU_INPROGRESS {
                panic(format_args!(
                    "sr_schedule_wu: work unit not in progress (state {})",
                    wu.swu_state.get()
                ));
            }

            // Walk queue backwards and fill in collider if we have one.
            let collider = sd.sd_wu_pendq.iter_reverse().find(|wup| {
                !(wu.swu_blk_end.get() < wup.swu_blk_start.get()
                    || wup.swu_blk_end.get() < wu.swu_blk_start.get())
            });
            if let Some(wup) = collider {
                // Defer work unit due to LBA collision.
                // DNPRINTF(SR_D_WU, "sr_schedule_wu: deferring work unit %p")
                wu.swu_state.set(SR_WU_DEFERRED);
                // SAFETY: work units on the pending queue live until `sr_wu_free`.
                let mut wup: &'static SrWorkunit = unsafe { &*ptr::from_ref(wup) };
                while let Some(next) = wup.swu_collider.get() {
                    wup = next;
                }
                wup.swu_collider.set(Some(wu));
                // SAFETY: an in-progress work unit is on no processing queue; at `splbio`.
                unsafe { sd.sd_wu_defq.insert_tail(wu) };
                sd.sd_wu_collisions.set(sd.sd_wu_collisions.get() + 1);
                break 'queued;
            }
        }

        // start:
        sr_raid_startwu(wu);
    }

    splx(s);
}

/// `sr_raid_startwu`: moves the work unit to the pending queue and starts its ccbs. At
/// `splbio`.
pub fn sr_raid_startwu(wu: &'static SrWorkunit) {
    let sd = wu.dis();

    // DNPRINTF(SR_D_WU, "sr_raid_startwu: start wu %p")

    splassert(IPL_BIO, "sr_raid_startwu");

    if wu.swu_state.get() == SR_WU_DEFERRED {
        // SAFETY: a deferred work unit is on the deferred queue (`sr_schedule_wu`,
        // `sr_rebuild`); at `splbio`.
        unsafe { sd.sd_wu_defq.remove(wu) };
        wu.swu_state.set(SR_WU_INPROGRESS);
    }

    if wu.swu_state.get() != SR_WU_RESTART {
        // SAFETY: the work unit is on no processing queue now; at `splbio`.
        unsafe { sd.sd_wu_pendq.insert_tail(wu) };
    }

    // Start all of the individual I/Os.
    if wu.swu_cb_active.get() == 1 {
        panic(format_args!("{}: sr_startwu_callback", DEVNAME(sd.sd_sc())));
    }
    wu.swu_cb_active.set(1);

    for ccb in wu.swu_ccb.iter() {
        // SAFETY: ccbs live until `sr_ccb_free`; the iterator borrows the `'static` unit.
        let ccb: &'static SrCcb = unsafe { &*ptr::from_ref(ccb) };
        let Some(vp) = ccb.ccb_buf.b_vp.get() else {
            panic(format_args!("{}: ccb without vnode", DEVNAME(sd.sd_sc())));
        };
        let _ = VOP_STRATEGY(vp, &ccb.ccb_buf);
    }

    wu.swu_cb_active.set(0);
}

/// `sr_raid_recreate_wu`: recreates a work unit by releasing the associated ccbs and
/// reissuing the SCSI I/O request. This process is then repeated for all of the colliding
/// work units.
pub fn sr_raid_recreate_wu(wu: &'static SrWorkunit) {
    let sd = wu.dis();
    let mut wup = Some(wu);

    while let Some(w) = wup {
        sr_wu_release_ccbs(w);

        w.swu_state.set(SR_WU_REQUEUE);
        if sd.sd_scsi_rw(w).is_err() {
            panic(format_args!("could not requeue I/O"));
        }

        wup = w.swu_collider.get();
    }
}

/// `sr_alloc_resources`: the default `sd_alloc_resources`: work units and ccbs.
pub fn sr_alloc_resources(sd: &'static SrDiscipline) -> Result<(), Errno> {
    if sr_wu_alloc(sd).is_err() {
        sr_error(sd.sd_sc(), format_args!("unable to allocate work units"));
        return Err(Errno::ENOMEM);
    }
    if sr_ccb_alloc(sd).is_err() {
        sr_error(sd.sd_sc(), format_args!("unable to allocate ccbs"));
        return Err(Errno::ENOMEM);
    }

    Ok(())
}

/// `sr_free_resources`: the default `sd_free_resources`.
pub fn sr_free_resources(sd: &'static SrDiscipline) {
    sr_wu_free(sd);
    sr_ccb_free(sd);
}

/// `sr_set_chunk_state`: the default `sd_set_chunk_state`: only online to offline.
pub fn sr_set_chunk_state(sd: &'static SrDiscipline, c: usize, new_state: i32) {
    // DNPRINTF(SR_D_STATE, "%s: %s: %s: sr_set_chunk_state %d -> %d")

    // ok to go to splbio since this only happens in error path
    let s = splbio();
    let chunk = sd.sd_vol.sv_chunk(c);
    let old_state = chunk.src_meta.scm_status.get() as i32;

    // multiple IOs to the same chunk that fail will come through here
    if old_state != new_state {
        if !(old_state == BIOC_SDONLINE && new_state == BIOC_SDOFFLINE) {
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
    splx(s);
}

/// `sr_set_vol_state`: the default `sd_set_vol_state`: online while every chunk is.
pub fn sr_set_vol_state(sd: &'static SrDiscipline) {
    let mut states = [0usize; SR_MAX_STATES];
    let old_state = sd.sd_vol_status.get();

    // DNPRINTF(SR_D_STATE, "%s: %s: sr_set_vol_state")

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

    let new_state = if states[BIOC_SDONLINE as usize] == nd {
        BIOC_SVONLINE
    } else {
        BIOC_SVOFFLINE
    };

    // DNPRINTF(SR_D_STATE, "%s: %s: sr_set_vol_state %d -> %d")

    // From offline (XXX this might be a little too much) and any other state: die.
    if old_state != BIOC_SVONLINE {
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

/// `sr_block_get`: `length` bytes of zeroed DMA memory (`dma_alloc(PR_NOWAIT | PR_ZERO)`),
/// `None` when there is none.
pub fn sr_block_get(_sd: &SrDiscipline, length: i64) -> Option<NonNull<u8>> {
    let len = usize::try_from(length).ok()?;
    malloc(len.max(1), M_DEVBUF, M_NOWAIT | M_ZERO)
}

/// `sr_block_put`: frees an [`sr_block_get`] block of `length` bytes.
pub fn sr_block_put(_sd: &SrDiscipline, ptr: NonNull<u8>, length: i64) {
    free(ptr, M_DEVBUF, usize::try_from(length).unwrap_or(0).max(1));
}

/// `sr_checksum_print` (`SR_DEBUG` callers only).
pub fn sr_checksum_print(md5: &[u8; MD5_DIGEST_LENGTH]) {
    for b in md5 {
        printf(format_args!("{:02x}", b));
    }
}

/// `sr_checksum`: the MD5 of `src`.
pub fn sr_checksum(_sc: &SrSoftc, src: &[Cell<u8>]) -> [u8; MD5_DIGEST_LENGTH] {
    // DNPRINTF(SR_D_MISC, "sr_checksum(%p %p %d)")

    let mut ctx = Md5Ctx::default();
    let mut md5 = [0u8; MD5_DIGEST_LENGTH];
    MD5Init(&mut ctx);
    let mut buf = [0u8; 64];
    for chunk in src.chunks(buf.len()) {
        cells_read(&mut buf[..chunk.len()], chunk);
        MD5Update(&mut ctx, &buf[..chunk.len()]);
    }
    MD5Final(&mut md5, &mut ctx);
    md5
}

/// `sr_uuid_generate`: a random (version 4, RFC 4122 variant) UUID.
pub fn sr_uuid_generate(uuid: &mut SrUuid) {
    arc4random_buf(&mut uuid.sui_id);
    // UUID version 4: random
    uuid.sui_id[6] &= 0x0f;
    uuid.sui_id[6] |= 0x40;
    // RFC4122 variant
    uuid.sui_id[8] &= 0x3f;
    uuid.sui_id[8] |= 0x80;
}

/// `sr_uuid_format`: the UUID as `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` and a NUL (the C
/// returns a `malloc`ed 37-byte string).
pub fn sr_uuid_format(uuid: &SrUuid) -> [u8; 37] {
    let u = &uuid.sui_id;
    let mut s = [0u8; 37];
    let _ = snprintf(
        &mut s,
        format_args!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-\
             {:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            u[0],
            u[1],
            u[2],
            u[3],
            u[4],
            u[5],
            u[6],
            u[7],
            u[8],
            u[9],
            u[10],
            u[11],
            u[12],
            u[13],
            u[14],
            u[15]
        ),
    );
    s
}

/// `sr_uuid_print`.
pub fn sr_uuid_print(uuid: &SrUuid, cr: bool) {
    let s = sr_uuid_format(uuid);
    printf(format_args!("{}{}", Str(&s), if cr { "\n" } else { "" }));
}

/// `sr_validate_stripsize`: the shift of a strip size that is a power of two multiple of
/// `DEV_BSIZE`, `None` otherwise (the C's -1).
pub fn sr_validate_stripsize(b: u32) -> Option<i32> {
    if b == 0 || !(b as usize).is_multiple_of(DEV_BSIZE) {
        return None;
    }

    let s = b.trailing_zeros();
    // only multiple of twos
    if b >> s != 1 {
        return None;
    }

    Some(s as i32)
}

/// `sr_validate_io`: checks a read or write of the volume (online, a data length, a CDB of
/// 6, 10 or 16 bytes, inside the volume, else sense data `ILLEGAL REQUEST`), records its
/// block range in the work unit and returns its first block (in `DEV_BSIZE` units).
pub fn sr_validate_io(wu: &'static SrWorkunit, func: &str) -> Result<Daddr, Errno> {
    let sd = wu.dis();
    let xs = wu.xs();
    let meta = sd.sd_meta();

    // DNPRINTF(SR_D_DIS, "%s 0x%02x")

    if meta.ssd_data_blkno.get() == 0 {
        panic(format_args!("invalid data blkno"));
    }

    if sd.sd_vol_status.get() == BIOC_SVOFFLINE {
        // DNPRINTF(SR_D_DIS, "%s device offline")
        return Err(Errno::EIO);
    }

    if xs.datalen() == 0 {
        printf(format_args!(
            "{}: {}: illegal block count for {}\n",
            DEVNAME(sd.sd_sc()),
            func,
            Name(meta.ssd_devname.get())
        ));
        return Err(Errno::EIO);
    }

    let mut blkno: Daddr = match xs.cmdlen.get() {
        10 => Daddr::from(_4btol(&xs.cmd_as::<ScsiRw10>().addr)),
        16 => _8btol(&xs.cmd_as::<ScsiRw16>().addr) as Daddr,
        6 => Daddr::from(_3btol(&xs.cmd_as::<ScsiRw>().addr)),
        _ => {
            printf(format_args!(
                "{}: {}: illegal cmdlen for {}\n",
                DEVNAME(sd.sd_sc()),
                func,
                Name(meta.ssd_devname.get())
            ));
            return Err(Errno::EIO);
        }
    };

    blkno *= Daddr::from(meta.ssdi().ssd_secsize.get() / DEV_BSIZE as u32);

    wu.swu_blk_start.set(blkno);
    wu.swu_blk_end
        .set(blkno + Daddr::from(xs.datalen() >> DEV_BSHIFT) - 1);

    if wu.swu_blk_end.get() > meta.ssdi().ssd_size.get() {
        // DNPRINTF(SR_D_DIS, "%s out of bounds start: %lld end: %lld length: %d")
        sr_set_sense(
            sd,
            SSD_ERRCODE_CURRENT | SSD_ERRCODE_VALID,
            SKEY_ILLEGAL_REQUEST,
            0x21,
            0x00,
        );
        return Err(Errno::EIO);
    }

    Ok(blkno)
}

/// `sr_rebuild_thread`: the rebuild kernel thread's body.
pub fn sr_rebuild_thread(arg: *mut c_void) {
    // SAFETY: `arg` is the discipline `sr_rebuild_start` created the thread for; the
    // discipline waits for the rebuild to stop (`sd_reb_active`) before it goes.
    let sd: &'static SrDiscipline = unsafe { &*arg.cast::<SrDiscipline>() };

    // DNPRINTF(SR_D_REBUILD, "%s: %s rebuild thread started")

    sd.sd_reb_active.set(1);
    sd.sd_rebuild();
    sd.sd_reb_active.set(0);

    kthread_exit(0);
}

/// A `scsi_xfer_pool` transfer, zeroed: the C's `struct scsi_xfer` locals of `sr_rebuild`.
fn sr_xs_alloc() -> Option<(NonNull<u8>, &'static ScsiXfer)> {
    let mem = pool_get(&SCSI_XFER_POOL, PR_WAITOK | PR_ZERO)?;
    let p = mem.cast::<ScsiXfer>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<ScsiXfer>()` bytes, written
    // once; it stays allocated until the `pool_put` of `sr_xs_free`.
    unsafe { p.as_ptr().write(ScsiXfer::new()) };
    // SAFETY: initialised above.
    Some((mem, unsafe { p.as_ref() }))
}

/// Gives an [`sr_xs_alloc`] transfer back.
fn sr_xs_free(mem: NonNull<u8>) {
    pool_put(&SCSI_XFER_POOL, mem);
}

/// Sets up a rebuild transfer: `READ_16` or `WRITE_16` of `lbasz` sectors at `lba`, `len`
/// bytes at `buf`.
fn sr_rebuild_xs(
    xs: &ScsiXfer,
    opcode: u8,
    flags: i32,
    lba: u64,
    lbasz: u32,
    buf: &mut DmaBuf,
    len: usize,
) {
    // bzero(&xs, sizeof xs): the members a transfer of the discipline reads or sets.
    xs.cmd.set(ScsiGeneric {
        opcode: 0,
        bytes: [0; 15],
    });
    xs.resid.set(0);
    xs.status.set(0);
    xs.sense.set(Default::default());
    xs.bp.set(None);
    xs.error.set(XS_NOERROR);
    xs.flags.set(flags);
    let data = buf.bytes();
    // SAFETY: `data` is the rebuild's DMA buffer, which `sr_rebuild` keeps until both
    // transfers of this block are complete (it waits for the write, which the read starts).
    unsafe { xs.set_data(data.as_mut_ptr(), len as i32) };
    xs.cmdlen.set(size_of::<ScsiRw16>() as i32);
    xs.with_cmd::<ScsiRw16, _>(|c| {
        c.opcode = opcode;
        _lto4b(lbasz, &mut c.length);
        _lto8b(lba, &mut c.addr);
    });
}

/// `sr_rebuild`: the default `sd_rebuild`: reads every block of the volume through the
/// discipline and writes it back (which goes to the chunk being rebuilt), saving the
/// progress in the metadata every percent; then brings the rebuilt chunk online.
pub fn sr_rebuild(sd: &'static SrDiscipline) {
    let sc = sd.sd_sc();
    let meta = sd.sd_meta();
    let sec_size = u64::from(meta.ssdi().ssd_secsize.get());
    let size = meta.ssdi().ssd_size.get() as u64;
    let whole_blk = size / SR_REBUILD_IO_SIZE;
    let partial_blk = size % SR_REBUILD_IO_SIZE;
    let mut percent;
    let mut old_percent = -1;

    let mut restart = meta.ssd_rebuild.get() as u64 / SR_REBUILD_IO_SIZE;
    if restart > whole_blk {
        printf(format_args!(
            "{}: bogus rebuild restart offset, starting from 0\n",
            DEVNAME(sc)
        ));
        restart = 0;
    }
    if restart != 0 {
        // XXX there is a hole here; there is a possibility that we had a restart however
        // the chunk that was supposed to be rebuilt is no longer valid; we can reach this
        // situation when a rebuild is in progress and the box crashes and on reboot the
        // rebuild chunk is different (like zero'd or replaced). We need to check the uuid of
        // the chunk that is being rebuilt to assert this.
        percent = sr_rebuild_percent(sd);
        printf(format_args!(
            "{}: resuming rebuild on {} at {}%\n",
            DEVNAME(sc),
            Name(meta.ssd_devname.get()),
            percent
        ));
    }

    // currently this is 64k therefore we can use dma_alloc
    let buflen = (SR_REBUILD_IO_SIZE as usize) << DEV_BSHIFT;
    let Some(mut buf) = DmaBuf::new(buflen, M_WAITOK) else {
        return;
    };
    let (Some((xs_r_mem, xs_r)), Some((xs_w_mem, xs_w))) = (sr_xs_alloc(), sr_xs_alloc()) else {
        return;
    };

    let mut wu_r: Option<&'static SrWorkunit> = None;
    let mut wu_w: Option<&'static SrWorkunit> = None;

    'fail: {
        let mut aborted = false;
        let mut blk = restart;
        while blk <= whole_blk {
            let mut sz = SR_REBUILD_IO_SIZE;
            if blk == whole_blk {
                if partial_blk == 0 {
                    break;
                }
                sz = partial_blk;
            }
            let lba = (blk * SR_REBUILD_IO_SIZE) / (sec_size / DEV_BSIZE as u64);
            let lbasz = ((sz << DEV_BSHIFT) / sec_size) as u32;
            let len = (sz as usize) << DEV_BSHIFT;

            // get some wu
            let (Some(r), Some(w)) = (sr_scsi_wu_get(sd, 0), sr_scsi_wu_get(sd, 0)) else {
                break 'fail;
            };
            wu_r = Some(r);
            wu_w = Some(w);

            // DNPRINTF(SR_D_REBUILD, "%s: %s rebuild wu_r %p, wu_w %p")

            // setup read io
            sr_rebuild_xs(xs_r, READ_16, SCSI_DATA_IN, lba, lbasz, &mut buf, len);
            r.swu_state.set(SR_WU_CONSTRUCT);
            r.swu_flags.set(r.swu_flags.get() | SR_WUF_REBUILD);
            r.swu_xs.set(Some(xs_r));
            if sd.sd_scsi_rw(r).is_err() {
                printf(format_args!("{}: could not create read io\n", DEVNAME(sc)));
                break 'fail;
            }

            // setup write io
            sr_rebuild_xs(xs_w, WRITE_16, SCSI_DATA_OUT, lba, lbasz, &mut buf, len);
            w.swu_state.set(SR_WU_CONSTRUCT);
            w.swu_flags
                .set(w.swu_flags.get() | SR_WUF_REBUILD | SR_WUF_WAKEUP);
            w.swu_xs.set(Some(xs_w));
            if sd.sd_scsi_rw(w).is_err() {
                printf(format_args!("{}: could not create write io\n", DEVNAME(sc)));
                break 'fail;
            }

            // collide with the read io so that we get automatically started when the read
            // is done
            w.swu_state.set(SR_WU_DEFERRED);
            r.swu_collider.set(Some(w));
            let s = splbio();
            // SAFETY: a work unit just taken from the pool is on no processing queue; at
            // `splbio`.
            unsafe { sd.sd_wu_defq.insert_tail(w) };
            splx(s);

            // DNPRINTF(SR_D_REBUILD, "%s: %s rebuild scheduling wu_r %p")

            r.swu_state.set(SR_WU_INPROGRESS);
            sr_schedule_wu(r);

            // wait for write completion
            let mut slept = false;
            while w.swu_flags.get() & SR_WUF_REBUILDIOCOMP == 0 {
                let _ = tsleep_nsec(ptr::from_ref(w), PRIBIO, "sr_rebuild", INFSLP);
                slept = true;
            }
            // yield if we didn't sleep
            if !slept {
                let _ = tsleep_nsec(ptr::from_ref(sc), PWAIT, "sr_yield", msec_to_nsec(1));
            }

            sr_scsi_wu_put(sd, r);
            sr_scsi_wu_put(sd, w);
            wu_r = None;
            wu_w = None;

            meta.ssd_rebuild
                .set((lba * (sec_size / DEV_BSIZE as u64)) as i64);

            // XXX - this should be based on size, not percentage.
            // save metadata every percent
            percent = sr_rebuild_percent(sd);
            if percent != old_percent && blk != whole_blk {
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
                aborted = true;
                break;
            }
            blk += 1;
        }

        if !aborted {
            // all done
            meta.ssd_rebuild.set(0);
            for c in 0..meta.ssdi().ssd_chunk_no.get() as usize {
                if sd.sd_vol.sv_chunk(c).src_meta.scm_status.get() == BIOC_SDREBUILD as u32 {
                    sd.sd_set_chunk_state(c, BIOC_SDONLINE);
                    break;
                }
            }
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

    // fail:
    if let Some(r) = wu_r {
        sr_scsi_wu_put(sd, r);
    }
    if let Some(w) = wu_w {
        sr_scsi_wu_put(sd, w);
    }
    sr_xs_free(xs_r_mem);
    sr_xs_free(xs_w_mem);
    drop(buf);
}

#[cfg(test)]
mod tests;
