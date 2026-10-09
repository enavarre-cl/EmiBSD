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
//! `softraid_crypto.c`: the CRYPTO discipline of softraid(4). A volume of one chunk whose
//! data is encrypted sector by sector with AES-XTS through crypto(9), the sector number being
//! the XTS tweak. The data keys (`scr_key`, one per 2^30 sectors) are made at creation from
//! random bytes and kept in the volume's optional metadata (`struct sr_meta_crypto`)
//! encrypted with the mask key (AES-256 ECB), together with an HMAC-SHA1 of the plain keys
//! under `SHA1(mask key)` that tells a right mask key from a wrong one. The mask key comes
//! from userland (bioctl(8) derives it from a passphrase, and the KDF hint it needs for that
//! is kept in the metadata too) or from a key disk, a chunk of its own whose optional
//! metadata holds it.
//!
//! Upstream: sys/dev/softraid_crypto.c @ 3ce1f3f79392
//!
//! Writes are encrypted from the transfer's buffer into the work unit's own `MAXPHYS`
//! buffer (`cr_dmabuf`), which the ccb then writes; reads are decrypted in place when the
//! ccb is done (`sr_crypto_done`). RAID 1C (`softraid_raid1c.rs`) reuses the functions
//! that take a `struct sr_crypto` (`*_internal`, [`sr_crypto_prepare`], ...).
//!
//! ## Deviations
//! - `struct sr_crypto_wu` is [`SrCryptoWuReq`] here: `softraidvar.rs`'s [`SrCryptoWu`]
//!   (the work unit and `cr_dmabuf`) followed by the request's preallocated descriptors
//!   (`cr_crp`), kept as the parts of an empty `Vec` between two I/Os, because a Rust
//!   [`Cryptop`] borrows its buffer and key and so is built per I/O over those descriptors
//!   (no allocation at I/O time, as in the C). Its `struct uio` and `struct iovec` are
//!   locals of that request.
//! - [`sr_crypto_prepare`] takes the call to make on the prepared request (the caller's
//!   `crypto_invoke(crwu->cr_crp)`) and returns its result with the work unit: the request
//!   borrows the I/O buffer and a copy of the key, which live only for that call.
//! - `sr_crypto_rw` and `sr_raid1c_rw`: when encrypting a write fails, the C completes the
//!   transfer (`sr_scsi_done`), still queues the write of the buffer, and returns the error,
//!   so that `sr_scsi_cmd` completes the transfer a second time. Here the error return
//!   alone fails the transfer (once, `XS_DRIVER_STUFFUP` in `sr_scsi_cmd`) and nothing is
//!   written.
//! - Hooks return `Result<(), Errno>`; the C's bare 1 is `Err(EIO)`. `sr_crypto_encrypt` and
//!   `sr_crypto_decrypt` return `Err(EIO)` for the C's 1 (key setup failed) and
//!   `Err(EINVAL)` for its -1 (unknown algorithm); their callers test for `EINVAL` where the
//!   C tests `== -1`. They and the HMAC work on cell views (`&[Cell<u8>]`) of the key arrays.
//! - `dma_alloc(MAXPHYS)` for `cr_dmabuf` is `malloc(9)` (`kern/dma_alloc.c` is not
//!   ported; both archs' `bus_dma` reach any kernel memory).
//! - User structures are copied in as bytes and decoded (`struct sr_crypto_kdfinfo` and
//!   `struct sr_crypto_kdfpair` are not `AbiPod`; the pair has padding). A KDF hint longer
//!   than the 140-byte union it is copied from (`genkdf.len` up to 256 is accepted) copies
//!   only the union: the C reads past the structure.
//! - `sr_crypto_set_key`'s boot key (`data`) is a slice; one shorter than the mask key is
//!   `EINVAL` (the C copies 32 bytes from the pointer).
//! - `sr_crypto_create_key_disk` allocates the fake discipline's metadata as a whole
//!   metadata area (`SR_META_SIZE * DEV_BSIZE`, as every discipline's `sd_meta` is) instead
//!   of `sizeof(struct sr_metadata)`; it frees the key disk's optional metadata item with its
//!   buffer, which holds the mask key, after zeroing it (the C frees the item and leaks the
//!   buffer).
//! - `sr_crypto_read_key_disk`, for the original key disk format (mask key in an
//!   `SR_OPT_CRYPTO` item), reads where the C reads: `omh + sizeof(struct sr_meta_opt_hdr)`
//!   is pointer arithmetic on a `struct sr_meta_opt_hdr *`, so `24 * 24` bytes into the
//!   item, not 24. The bytes are kept on-disk compatible with OpenBSD's.
//! - `sr_crypto_free_resources_internal` clears `key_disk` after freeing it (the C leaves the
//!   pointer dangling) and zeroes it through its cells.
//! - A missing crypto metadata item (`scr_meta`), which the C dereferences as NULL in
//!   `sr_crypto_get_kdf`, `sr_crypto_decrypt_key`, `sr_crypto_change_maskkey`,
//!   `sr_crypto_alloc_resources_internal` and `sr_crypto_ioctl_internal`, makes them fail.
//! - A write reaching `sr_crypto_dev_rw` without its crypto work unit panics (the C
//!   dereferences NULL); writing the plain text instead is never done.
//! - `DNPRINTF` calls and `sr_crypto_dumpkeys` (`SR_DEBUG0`) are not configured; the
//!   `DNPRINTF`s are comments. `HIBERNATE` (`lib/libsa/aes_xts.h`, softraid.c's
//!   `sr_hibernate_io`) is not configured.

use alloc::vec::Vec;
use core::cell::Cell;
use core::ffi::c_void;
use core::mem::{ManuallyDrop, offset_of};
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{Ordering, compiler_fence};

use libkern::explicit_bzero;

use crate::crypto::crypto::{crypto_freesession, crypto_getreq, crypto_invoke, crypto_newsession};
use crate::crypto::cryptodev::{
    CRD_F_ENCRYPT, CRD_F_IV_EXPLICIT, CRD_F_IV_PRESENT, CRYPTO_AES_XTS, CRYPTO_F_IOV, CryptoBuf,
    Cryptodesc, Cryptoini, Cryptop, RIJNDAEL128_BLOCK_LEN,
};
use crate::crypto::hmac::HmacSha1Ctx;
use crate::crypto::rijndael::{
    AES_MAXKEYBYTES, RijndaelCtx, rijndael_decrypt, rijndael_encrypt, rijndael_set_key,
    rijndael_set_key_enc_only,
};
use crate::crypto::sha1::{SHA1_DIGEST_LENGTH, Sha1Ctx};
use crate::dev::biovar::{
    BIOC_SCNOAUTOASSEMBLE, BIOC_SDINVALID, BIOC_SDOFFLINE, BIOC_SDONLINE, BIOC_SOIN,
    BIOC_SOINOUT_FAILED, BIOC_SOINOUT_OK, BIOC_SOOUT, BIOC_SVONLINE, BiocCreateraid,
    BiocDiscipline,
};
use crate::dev::rnd::arc4random_buf;
use crate::dev::softraid::{
    SR_META_BYTES, SrMetaBuf, sr_ccb_alloc, sr_ccb_free, sr_ccb_rw, sr_checksum, sr_chunk_in_use,
    sr_error, sr_hotplug_register, sr_hotplug_unregister, sr_meta_chunk_at, sr_meta_getdevname,
    sr_meta_native_read, sr_meta_opt_load, sr_meta_save, sr_meta_validate, sr_schedule_wu,
    sr_scsi_done, sr_strlcpy_cell, sr_validate_io, sr_wu_alloc, sr_wu_enqueue_ccb, sr_wu_free,
};
use crate::dev::softraidvar::*;
use crate::kassert;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::{panic, printf};
use crate::kern::vfs_subr::{bdevvp, vput};
use crate::kern::vfs_vops::{VOP_CLOSE, VOP_IOCTL, VOP_OPEN};
use crate::machine::copy::{copyin, copyout};
use crate::machine::cpu::curproc;
use crate::machine::intr::{splbio, splx};
use crate::scsi::scsiconf::{SCSI_DATA_IN, SCSI_DATA_OUT, XS_DRIVER_STUFFUP, XS_NOERROR};
use crate::sys::disk::Disk;
use crate::sys::disklabel::{Disklabel, FS_RAID, diskpart};
use crate::sys::dkio::DIOCGDINFO;
use crate::sys::errno::Errno;
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::malloc::{M_CANFAIL, M_DEVBUF, M_WAITOK};
use crate::sys::param::{DEV_BSHIFT, DEV_BSIZE, MAXPHYS, NODEV};
use crate::sys::types::{Daddr, Dev};
use crate::sys::ucred::NOCRED;
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::vnode::Vnode;

/// The number of descriptors a request of `MAXPHYS` bytes needs (one per sector).
const SR_CRYPTO_NDESC: usize = MAXPHYS >> DEV_BSHIFT;

/// The size of the key arrays (`sizeof(mdd_crypto->scr_key)`, `scm_key`).
const SR_CRYPTO_KEYSZ: usize = SR_CRYPTO_MAXKEYS * SR_CRYPTO_KEYBYTES;

/// The size of `struct sr_crypto_kdfinfo` in user space.
const KDFINFO_SIZE: usize = size_of::<SrCryptoKdfinfo>();

/// The size of the KDF hint union of `struct sr_crypto_kdfinfo` (`&kdfinfo->genkdf`).
const KDFHINT_UNION_SIZE: usize = size_of::<SrCryptoPbkdf>();

/// The size of `struct sr_crypto_kdfpair` in user space.
const KDFPAIR_SIZE: usize = size_of::<SrCryptoKdfpair>();

/// `struct sr_crypto_wu` as the crypto disciplines allocate it: the header's [`SrCryptoWu`]
/// (the work unit, first, and `cr_dmabuf`) followed by `cr_crp`, the descriptors of the
/// request preallocated by `sr_crypto_alloc_resources_internal`
/// (`crypto_getreq(MAXPHYS >> DEV_BSHIFT)`) and kept empty between two I/Os.
#[repr(C)]
pub struct SrCryptoWuReq {
    /// The work unit and `cr_dmabuf`.
    pub cr: SrCryptoWu,
    /// The descriptors' allocation (`crp_desc`), NULL while none is held.
    cr_desc: Cell<*mut Cryptodesc<'static>>,
    /// `crp_ndescalloc`: the capacity of `cr_desc`.
    cr_ndescalloc: Cell<usize>,
}

// SAFETY: a zero-valid work unit extension, a null raw pointer and an integer cell; no
// `Drop` (the descriptors are released by `sr_crypto_free_resources_internal`).
unsafe impl SrZeroed for SrCryptoWuReq {}
// SAFETY: `#[repr(C)]` with the work unit (inside `SrCryptoWu`, its first member) first;
// `SrZeroed`.
unsafe impl SrWorkunitExt for SrCryptoWuReq {}

impl SrCryptoWuReq {
    /// `cr_dmabuf`.
    pub fn cr_dmabuf(&self) -> *mut u8 {
        self.cr.cr_dmabuf.get()
    }

    /// Takes the preallocated descriptors (empty) for a request; `None` when the work unit
    /// holds none.
    fn crp_take<'a>(&self) -> Option<Vec<Cryptodesc<'a>>> {
        let p = NonNull::new(self.cr_desc.replace(ptr::null_mut()))?;
        // SAFETY: `cr_desc` and `cr_ndescalloc` are the pointer and capacity of a vector of
        // `Cryptodesc`s that `crp_put` gave up (ManuallyDrop) when it was empty; clearing
        // `cr_desc` above makes this the only vector rebuilt from them. A lifetime does not
        // change the layout of `Cryptodesc`, and the vector holds no value.
        Some(unsafe {
            Vec::from_raw_parts(
                p.as_ptr().cast::<Cryptodesc<'a>>(),
                0,
                self.cr_ndescalloc.get(),
            )
        })
    }

    /// Gives the descriptors back, emptied, their allocation kept for the next request.
    fn crp_put(&self, descs: Vec<Cryptodesc<'_>>) {
        let mut descs = ManuallyDrop::new(descs);
        descs.clear();
        self.cr_ndescalloc.set(descs.capacity());
        self.cr_desc
            .set(descs.as_mut_ptr().cast::<Cryptodesc<'static>>());
    }

    /// `crypto_freereq(crwu->cr_crp)`.
    fn crp_free(&self) {
        drop(self.crp_take());
    }
}

/// The members of a `struct sr_crypto_kdfpair` copied in from user space (user addresses).
struct KdfPair {
    /// `kdfinfo1`.
    kdfinfo1: usize,
    /// `kdfsize1`.
    kdfsize1: u32,
    /// `kdfinfo2`.
    kdfinfo2: usize,
    /// `kdfsize2`.
    kdfsize2: u32,
}

/// `sr_crypto_discipline_init`: discipline initialisation.
pub fn sr_crypto_discipline_init(sd: &'static SrDiscipline) {
    // Fill out discipline members.
    sd.set_wu_type::<SrCryptoWuReq>();
    sd.sd_type.set(SR_MD_CRYPTO);
    sr_strlcpy_cell(&sd.sd_name, b"CRYPTO");
    sd.sd_capabilities
        .set(SR_CAP_SYSTEM_DISK | SR_CAP_AUTO_ASSEMBLE);
    sd.sd_max_wu.set(SR_CRYPTO_NOWU);

    for sid in &sd.mds().mdd_crypto.scr_sid {
        sid.set(u64::MAX);
    }

    // Setup discipline specific function pointers.
    sd.sd_alloc_resources.set(Some(sr_crypto_alloc_resources));
    sd.sd_assemble.set(Some(sr_crypto_assemble));
    sd.sd_create.set(Some(sr_crypto_create));
    sd.sd_free_resources.set(Some(sr_crypto_free_resources));
    sd.sd_ioctl_handler.set(Some(sr_crypto_ioctl));
    sd.sd_meta_opt_handler.set(Some(sr_crypto_meta_opt_handler));
    sd.sd_scsi_rw.set(Some(sr_crypto_rw));
    sd.sd_scsi_done.set(Some(sr_crypto_done));
}

/// `sr_crypto_create`: sets up a new CRYPTO volume of exactly one chunk of `coerced_size`
/// blocks, its crypto metadata and keys.
pub fn sr_crypto_create(
    sd: &'static SrDiscipline,
    bc: &mut BiocCreateraid,
    no_chunk: i32,
    coerced_size: i64,
) -> Result<(), Errno> {
    if no_chunk != 1 {
        sr_error(
            sd.sd_sc(),
            format_args!("{} requires exactly one chunk", sd.name()),
        );
        return Err(Errno::EINVAL);
    }

    sd.sd_meta().ssdi().ssd_size.set(coerced_size);

    sr_crypto_meta_create(sd, &sd.mds().mdd_crypto, bc)?;

    sd.sd_max_ccb_per_wu.set(no_chunk as u32);
    Ok(())
}

/// `sr_crypto_meta_create`: adds the crypto optional metadata to a new volume, gets the mask
/// key (from userland, or a new key disk), and makes and masks the data keys.
pub fn sr_crypto_meta_create(
    sd: &'static SrDiscipline,
    mdd_crypto: &SrCrypto,
    bc: &mut BiocCreateraid,
) -> Result<(), Errno> {
    let size = sd.sd_meta().ssdi().ssd_size.get();
    if size as u64 > SR_CRYPTO_MAXSIZE {
        sr_error(
            sd.sd_sc(),
            format_args!(
                "{} exceeds maximum size ({} > {})",
                sd.name(),
                size,
                SR_CRYPTO_MAXSIZE
            ),
        );
        return Err(Errno::EINVAL);
    }

    // Create crypto optional metadata.
    let Some(omi) = SrMetaOptItem::alloc(size_of::<SrMetaCrypto>(), M_WAITOK) else {
        return Err(Errno::ENOMEM);
    };
    let som = omi.omi_som();
    som.som_type.set(SR_OPT_CRYPTO);
    som.som_length.set(size_of::<SrMetaCrypto>() as u32);
    // SAFETY: a new item on no list; the discipline's list owns it until
    // `sr_discipline_free`.
    unsafe { sd.sd_meta_opt.insert_head(omi) };
    mdd_crypto.scr_meta.set(
        omi.som_as::<SrMetaCrypto>()
            .map_or(ptr::null(), ptr::from_ref),
    );
    let ssdi = sd.sd_meta().ssdi();
    ssdi.ssd_opt_no.set(ssdi.ssd_opt_no.get() + 1);

    mdd_crypto.key_disk.set(None);

    if bc.bc_key_disk != NODEV {
        // Create a key disk.
        sr_crypto_get_kdf(bc, sd, mdd_crypto)?;
        let key_disk = sr_crypto_create_key_disk(sd, mdd_crypto, bc.bc_key_disk);
        if key_disk.is_none() {
            return Err(Errno::EINVAL);
        }
        mdd_crypto.key_disk.set(key_disk);
        sd.sd_capabilities
            .set(sd.sd_capabilities.get() | SR_CAP_AUTO_ASSEMBLE);
    } else if bc.bc_opaque_flags & BIOC_SOOUT != 0 {
        // No hint available yet.
        bc.bc_opaque_status = BIOC_SOINOUT_FAILED;
        return Err(Errno::EAGAIN);
    } else {
        sr_crypto_get_kdf(bc, sd, mdd_crypto)?;
    }

    // Passphrase volumes cannot be automatically assembled.
    if bc.bc_flags & BIOC_SCNOAUTOASSEMBLE == 0 && bc.bc_key_disk == NODEV {
        return Err(Errno::EINVAL);
    }

    let _ = sr_crypto_create_keys(sd, mdd_crypto);

    Ok(())
}

/// `sr_crypto_set_key`: gets the mask key of an existing volume: the boot loader's `data`,
/// the key disk, or userland (which first asks for the KDF hint: `EAGAIN`).
pub fn sr_crypto_set_key(
    sd: &'static SrDiscipline,
    mdd_crypto: &SrCrypto,
    bc: &mut BiocCreateraid,
    _no_chunk: i32,
    data: Option<&[u8]>,
) -> Result<(), Errno> {
    mdd_crypto.key_disk.set(None);

    // Crypto optional metadata must already exist...
    let Some(meta) = mdd_crypto.scr_meta() else {
        return Err(Errno::EINVAL);
    };

    if let Some(data) = data {
        // Kernel already has mask key.
        let Some(key) = data.get(..SR_CRYPTO_MAXKEYBYTES) else {
            return Err(Errno::EINVAL);
        };
        let mut maskkey = [0u8; SR_CRYPTO_MAXKEYBYTES];
        maskkey.copy_from_slice(key);
        mdd_crypto.scr_maskkey.set(maskkey);
        explicit_bzero(&mut maskkey);
    } else if bc.bc_key_disk != NODEV {
        // Read the mask key from the key disk.
        let key_disk = sr_crypto_read_key_disk(sd, mdd_crypto, bc.bc_key_disk);
        if key_disk.is_none() {
            return Err(Errno::EINVAL);
        }
        mdd_crypto.key_disk.set(key_disk);
    } else if bc.bc_opaque_flags & BIOC_SOOUT != 0 {
        // provide userland with kdf hint
        if bc.bc_opaque.is_null() {
            return Err(Errno::EINVAL);
        }

        let hint = meta.scm_kdfhint.get();
        let Some(hint) = hint.get(..bc.bc_opaque_size as usize) else {
            return Err(Errno::EINVAL);
        };

        if copyout(hint, bc.bc_opaque as usize).is_err() {
            return Err(Errno::EINVAL);
        }

        // we're done
        bc.bc_opaque_status = BIOC_SOINOUT_OK;
        return Err(Errno::EAGAIN);
    } else if bc.bc_opaque_flags & BIOC_SOIN != 0 {
        // get kdf with maskkey from userland
        sr_crypto_get_kdf(bc, sd, mdd_crypto)?;
    } else {
        return Err(Errno::EINVAL);
    }

    Ok(())
}

/// `sr_crypto_assemble`: brings up an existing volume once its mask key is known.
pub fn sr_crypto_assemble(
    sd: &'static SrDiscipline,
    bc: &mut BiocCreateraid,
    no_chunk: i32,
    data: Option<&[u8]>,
) -> Result<(), Errno> {
    sr_crypto_set_key(sd, &sd.mds().mdd_crypto, bc, no_chunk, data)?;

    sd.sd_max_ccb_per_wu
        .set(sd.sd_meta().ssdi().ssd_chunk_no.get());
    Ok(())
}

/// The request `sr_crypto_prepare` makes over `buf`, a run of whole sectors starting at
/// sector `blkno`: one AES-XTS descriptor per sector, the sector number (in the machine's
/// byte order, as the C's `memcpy` of a `daddr_t`) as its explicit IV, the session of the
/// key that covers `blkno`. `invoke` is called on it (the caller's `crypto_invoke`); `None`
/// when the work unit holds no descriptors.
fn sr_crypto_request(
    crwu: &SrCryptoWuReq,
    mdd_crypto: &SrCrypto,
    buf: &mut [u8],
    blkno: Daddr,
    encrypt: bool,
    invoke: impl FnOnce(&mut Cryptop<'_>) -> Result<(), Errno>,
) -> Option<Result<(), Errno>> {
    let datalen = buf.len();
    let n = datalen >> DEV_BSHIFT;

    // We preallocated enough crypto descs for up to MAXPHYS of I/O. Since there may be less
    // than that we need to tweak the amount of crypto desc structures to be just long enough
    // for our needs.
    let mut descs = crwu.crp_take()?;
    kassert!(descs.capacity() >= n);
    let flags = (if encrypt { CRD_F_ENCRYPT } else { 0 }) | CRD_F_IV_PRESENT | CRD_F_IV_EXPLICIT;

    // Select crypto session based on block number.
    //
    // XXX - this does not handle the case where the read/write spans across a different key
    // blocks (e.g. 0.5TB boundary). Currently this is already broken by the use of scr_key[0]
    // below.
    let keyndx = (blkno >> SR_CRYPTO_KEY_BLKSHIFT) as usize;
    let sid = mdd_crypto.scr_sid.get(keyndx).map_or(u64::MAX, Cell::get);

    let mut key = mdd_crypto.scr_key[0].get();
    for i in 0..n {
        let mut crd = Cryptodesc {
            crd_skip: (i << DEV_BSHIFT) as i32,
            crd_len: DEV_BSIZE as i32,
            crd_inject: 0,
            crd_flags: flags,
            CRD_INI: Cryptoini {
                cri_alg: mdd_crypto.scr_alg.get(),
                cri_klen: mdd_crypto.scr_klen.get(),
                cri_key: &key,
                ..Cryptoini::default()
            },
        };
        let b = blkno + i as Daddr;
        crd.crd_iv_mut()[..size_of::<Daddr>()].copy_from_slice(&b.to_ne_bytes());
        descs.push(crd);
    }

    let mut iov = [Iovec {
        iov_base: buf.as_mut_ptr().cast::<c_void>(),
        iov_len: datalen,
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: datalen,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    };
    let ndescalloc = descs.capacity() as i32;
    let mut crp = Cryptop {
        crp_sid: sid,
        crp_ilen: datalen as i32,
        crp_olen: 0,
        crp_alloctype: M_DEVBUF,
        crp_flags: CRYPTO_F_IOV,
        crp_buf: CryptoBuf::Iov(&mut uio),
        crp_desc: descs,
        crp_ndesc: n as i32,
        crp_ndescalloc: ndescalloc,
        crp_mac: None,
    };

    let rv = invoke(&mut crp);

    let descs = core::mem::take(&mut crp.crp_desc);
    drop(crp);
    crwu.crp_put(descs);
    explicit_bzero(&mut key);

    Some(rv)
}

/// `sr_crypto_prepare`: the work unit's crypto request over its transfer, sector by sector
/// from `swu_blk_start`; a write's data is first copied into `cr_dmabuf` and encrypted
/// there, a read's is decrypted in place. The C returns the work unit with its `cr_crp` set
/// up for the caller's `crypto_invoke`; here the request borrows the buffer and the key for
/// one call, so the caller passes that call (`invoke`) and gets its result with the work
/// unit.
pub fn sr_crypto_prepare(
    wu: &'static SrWorkunit,
    mdd_crypto: &SrCrypto,
    encrypt: bool,
    invoke: impl FnOnce(&mut Cryptop<'_>) -> Result<(), Errno>,
) -> (&'static SrCryptoWuReq, Result<(), Errno>) {
    let xs = wu.xs();
    let crwu = sr_wu_ext::<SrCryptoWuReq>(wu);

    // DNPRINTF(SR_D_DIS, "%s: sr_crypto_prepare wu %p encrypt %d\n")

    let datalen = usize::try_from(xs.datalen()).unwrap_or(0);
    let buf: &mut [u8] = if xs.flags.get() & SCSI_DATA_OUT != 0 {
        let dmabuf = crwu.cr_dmabuf();
        if dmabuf.is_null() || datalen > SR_CRYPTO_DMABUF_SIZE {
            return (crwu, Err(Errno::ENOMEM));
        }
        // SAFETY: `cr_dmabuf` is this work unit's `MAXPHYS` allocation
        // (`sr_crypto_alloc_resources_internal`), used only by the work unit's current I/O;
        // `datalen` fits (checked above).
        let dma = unsafe { slice::from_raw_parts_mut(dmabuf, datalen) };
        // SAFETY: the discipline owns the transfer between `sr_scsi_cmd` and `sr_scsi_done`
        // and holds no other slice of its data.
        let data = unsafe { xs.data_slice() };
        let len = data.len().min(datalen);
        dma[..len].copy_from_slice(&data[..len]);
        dma
    } else {
        // SAFETY: as above; the read's data is decrypted in place.
        unsafe { xs.data_slice() }
    };

    let blkno = wu.swu_blk_start.get();
    let rv = sr_crypto_request(crwu, mdd_crypto, buf, blkno, encrypt, invoke)
        .unwrap_or(Err(Errno::ENOMEM));
    (crwu, rv)
}

/// `sr_crypto_get_kdf`: copies in userland's `struct sr_crypto_kdfinfo` (`bc_opaque`): its
/// KDF hint goes to the metadata, its mask key to `scr_maskkey`.
pub fn sr_crypto_get_kdf(
    bc: &mut BiocCreateraid,
    _sd: &SrDiscipline,
    mdd_crypto: &SrCrypto,
) -> Result<(), Errno> {
    if bc.bc_opaque_flags & BIOC_SOIN == 0 {
        return Err(Errno::EINVAL);
    }
    if bc.bc_opaque.is_null() {
        return Err(Errno::EINVAL);
    }
    if bc.bc_opaque_size as usize != KDFINFO_SIZE {
        return Err(Errno::EINVAL);
    }

    let mut raw = [0u8; KDFINFO_SIZE];
    let mut kdfinfo = SrCryptoKdfinfo::default();
    let rv = 'out: {
        if copyin(bc.bc_opaque as usize, &mut raw).is_err() {
            break 'out Err(Errno::EINVAL);
        }
        kdfinfo = kdfinfo_from_bytes(&raw);

        if kdfinfo.len != bc.bc_opaque_size {
            break 'out Err(Errno::EINVAL);
        }

        // copy KDF hint to disk meta data
        if kdfinfo.flags & SR_CRYPTOKDF_HINT != 0 {
            let Some(meta) = mdd_crypto.scr_meta() else {
                break 'out Err(Errno::EINVAL);
            };
            if SR_CRYPTO_KDFHINTBYTES < kdfinfo.genkdf().len as usize {
                break 'out Err(Errno::EINVAL);
            }
            kdfhint_copy(meta, &raw, kdfinfo.genkdf().len as usize);
        }

        // copy mask key to run-time meta data
        if kdfinfo.flags & SR_CRYPTOKDF_KEY != 0 {
            // sizeof(scr_maskkey) == sizeof(kdfinfo->maskkey)
            mdd_crypto.scr_maskkey.set(kdfinfo.maskkey);
        }

        bc.bc_opaque_status = BIOC_SOINOUT_OK;
        Ok(())
    };

    explicit_bzero(&mut raw);
    kdfinfo_bzero(&mut kdfinfo);

    rv
}

/// `sr_crypto_encrypt`: encrypts the plain text `p` into `c` (as long as `p`) under the
/// 256-bit `key` with `alg` (`SR_CRYPTOM_AES_ECB_256`). `Err(EIO)` is the C's 1 (bad key),
/// `Err(EINVAL)` its -1 (unknown algorithm).
pub fn sr_crypto_encrypt(
    p: &[Cell<u8>],
    c: &[Cell<u8>],
    key: &[u8],
    alg: u32,
) -> Result<(), Errno> {
    let mut ctx = RijndaelCtx::default();

    let rv = match alg {
        SR_CRYPTOM_AES_ECB_256 => {
            if rijndael_set_key_enc_only(&mut ctx, key, 256).is_err() {
                Err(Errno::EIO)
            } else {
                ecb_blocks(p, c, |src, dst| rijndael_encrypt(&ctx, src, dst));
                Ok(())
            }
        }
        _ => {
            // DNPRINTF(SR_D_DIS, "%s: unsupported encryption algorithm %d\n")
            Err(Errno::EINVAL)
        }
    };

    ctx_bzero(&mut ctx);
    rv
}

/// `sr_crypto_decrypt`: decrypts `c` into `p`; see [`sr_crypto_encrypt`].
pub fn sr_crypto_decrypt(
    c: &[Cell<u8>],
    p: &[Cell<u8>],
    key: &[u8],
    alg: u32,
) -> Result<(), Errno> {
    let mut ctx = RijndaelCtx::default();

    let rv = match alg {
        SR_CRYPTOM_AES_ECB_256 => {
            if rijndael_set_key(&mut ctx, key, 256).is_err() {
                Err(Errno::EIO)
            } else {
                ecb_blocks(c, p, |src, dst| rijndael_decrypt(&ctx, src, dst));
                Ok(())
            }
        }
        _ => {
            // DNPRINTF(SR_D_DIS, "%s: unsupported encryption algorithm %d\n")
            Err(Errno::EINVAL)
        }
    };

    ctx_bzero(&mut ctx);
    rv
}

/// `sr_crypto_calculate_check_hmac_sha1`: `HMAC-SHA1_k(key)` with `k = SHA1(maskkey)`, the
/// check that tells whether the data keys were decrypted with the right mask key.
pub fn sr_crypto_calculate_check_hmac_sha1(
    maskkey: &[u8],
    key: &[Cell<u8>],
    check_digest: &mut [u8; SHA1_DIGEST_LENGTH],
) {
    let mut shactx = Sha1Ctx::new();
    let mut chunk = [0u8; 64];

    // k = SHA1(mask_key)
    shactx.update(maskkey);
    let mut check_key = shactx.finalize();

    // mac = HMAC_SHA1_k(unencrypted key)
    let mut hmacctx = HmacSha1Ctx::new(&check_key);
    for part in key.chunks(chunk.len()) {
        let bytes = &mut chunk[..part.len()];
        cells_read(bytes, part);
        hmacctx.update(bytes);
    }
    *check_digest = hmacctx.finalize();

    explicit_bzero(&mut check_key);
    explicit_bzero(&mut chunk);
    // SAFETY: `hmacctx` and `shactx` are locals borrowed exclusively here; volatile stores of
    // their default (zero) values are not removed (the C's `explicit_bzero`).
    unsafe {
        ptr::write_volatile(&mut hmacctx, HmacSha1Ctx::default());
        ptr::write_volatile(&mut shactx, Sha1Ctx::default());
    }
}

/// `sr_crypto_decrypt_key`: decrypts the data keys from the metadata with the mask key and
/// checks them against the stored HMAC; the mask key is forgotten either way.
pub fn sr_crypto_decrypt_key(_sd: &SrDiscipline, mdd_crypto: &SrCrypto) -> Result<(), Errno> {
    let mut check_digest = [0u8; SHA1_DIGEST_LENGTH];
    let mut maskkey = mdd_crypto.scr_maskkey.get();

    // DNPRINTF(SR_D_DIS, "%s: sr_crypto_decrypt_key\n")

    let rv = 'out: {
        let Some(meta) = mdd_crypto.scr_meta() else {
            break 'out Err(Errno::EIO);
        };
        if meta.scm_check_alg.get() != SR_CRYPTOC_HMAC_SHA1 {
            break 'out Err(Errno::EIO);
        }

        if sr_crypto_decrypt(
            key_cells(&meta.scm_key),
            key_cells(&mdd_crypto.scr_key),
            &maskkey,
            meta.scm_mask_alg.get(),
        ) == Err(Errno::EINVAL)
        {
            break 'out Err(Errno::EIO);
        }

        // Check that the key decrypted properly.
        sr_crypto_calculate_check_hmac_sha1(
            &maskkey,
            key_cells(&mdd_crypto.scr_key),
            &mut check_digest,
        );
        if meta.chk_hmac_sha1().sch_mac != check_digest {
            cells_bzero(key_cells(&mdd_crypto.scr_key));
            break 'out Err(Errno::EIO);
        }

        Ok(()) // Success
    };

    // we don't need the mask key anymore
    mdd_crypto.scr_maskkey.set([0; SR_CRYPTO_MAXKEYBYTES]);
    explicit_bzero(&mut maskkey);
    explicit_bzero(&mut check_digest);

    rv
}

/// `sr_crypto_create_keys`: makes random data keys (AES-XTS-256), stores them masked with
/// the mask key (AES-256 ECB) and their HMAC check in the metadata, and forgets them.
pub fn sr_crypto_create_keys(_sd: &SrDiscipline, mdd_crypto: &SrCrypto) -> Result<(), Errno> {
    // DNPRINTF(SR_D_DIS, "%s: sr_crypto_create_keys\n")

    if AES_MAXKEYBYTES < SR_CRYPTO_MAXKEYBYTES {
        return Err(Errno::EIO);
    }
    let Some(meta) = mdd_crypto.scr_meta() else {
        return Err(Errno::EIO);
    };
    let mut maskkey = mdd_crypto.scr_maskkey.get();

    // XXX allow user to specify
    meta.scm_alg.set(SR_CRYPTOA_AES_XTS_256);

    // generate crypto keys
    let mut key = [0u8; SR_CRYPTO_KEYBYTES];
    for k in &mdd_crypto.scr_key {
        arc4random_buf(&mut key);
        k.set(key);
    }
    explicit_bzero(&mut key);

    // Mask the disk keys.
    meta.scm_mask_alg.set(SR_CRYPTOM_AES_ECB_256);
    let _ = sr_crypto_encrypt(
        key_cells(&mdd_crypto.scr_key),
        key_cells(&meta.scm_key),
        &maskkey,
        meta.scm_mask_alg.get(),
    );

    // Prepare key decryption check code.
    meta.scm_check_alg.set(SR_CRYPTOC_HMAC_SHA1);
    let mut chk = SrCryptoChkHmacSha1::default();
    sr_crypto_calculate_check_hmac_sha1(&maskkey, key_cells(&mdd_crypto.scr_key), &mut chk.sch_mac);
    meta.set_chk_hmac_sha1(&chk);

    // Erase the plaintext disk keys
    cells_bzero(key_cells(&mdd_crypto.scr_key));
    explicit_bzero(&mut maskkey);

    meta.scm_flags.set(SR_CRYPTOF_KEY | SR_CRYPTOF_KDFHINT);

    Ok(())
}

/// `sr_crypto_change_maskkey`: re-masks the data keys with the mask key of `kdfinfo2` once
/// the one of `kdfinfo1` is shown right (`EPERM` otherwise), and stores the new KDF hint.
/// Both mask keys are forgotten.
pub fn sr_crypto_change_maskkey(
    sd: &SrDiscipline,
    mdd_crypto: &SrCrypto,
    kdfinfo1: &mut SrCryptoKdfinfo,
    kdfinfo2: &mut SrCryptoKdfinfo,
) -> Result<(), Errno> {
    let mut check_digest = [0u8; SHA1_DIGEST_LENGTH];
    let ksz = SR_CRYPTO_KEYSZ;
    let mut p: Option<SrMetaBuf> = None;

    // DNPRINTF(SR_D_DIS, "%s: sr_crypto_change_maskkey\n")

    let rv = 'out: {
        let Some(meta) = mdd_crypto.scr_meta() else {
            break 'out Err(Errno::EIO);
        };
        if meta.scm_check_alg.get() != SR_CRYPTOC_HMAC_SHA1 {
            break 'out Err(Errno::EIO);
        }

        let c = key_cells(&meta.scm_key);
        let Some(pb) = SrMetaBuf::new(ksz, M_WAITOK | M_CANFAIL) else {
            break 'out Err(Errno::EIO);
        };
        let pc = p.insert(pb).cells();

        if sr_crypto_decrypt(c, pc, &kdfinfo1.maskkey, meta.scm_mask_alg.get())
            == Err(Errno::EINVAL)
        {
            break 'out Err(Errno::EIO);
        }

        sr_crypto_calculate_check_hmac_sha1(&kdfinfo1.maskkey, pc, &mut check_digest);
        if meta.chk_hmac_sha1().sch_mac != check_digest {
            sr_error(sd.sd_sc(), format_args!("incorrect key or passphrase"));
            break 'out Err(Errno::EPERM);
        }

        // Copy new KDF hint to metadata, if supplied.
        if kdfinfo2.flags & SR_CRYPTOKDF_HINT != 0 {
            let len = kdfinfo2.genkdf().len as usize;
            if len > SR_CRYPTO_KDFHINTBYTES {
                break 'out Err(Errno::EIO);
            }
            meta.scm_kdfhint.set([0; SR_CRYPTO_KDFHINTBYTES]);
            let mut raw = kdfinfo_to_bytes(kdfinfo2);
            kdfhint_copy(meta, &raw, len);
            explicit_bzero(&mut raw);
        }

        // Mask the disk keys.
        if sr_crypto_encrypt(pc, c, &kdfinfo2.maskkey, meta.scm_mask_alg.get())
            == Err(Errno::EINVAL)
        {
            break 'out Err(Errno::EIO);
        }

        // Prepare key decryption check code.
        meta.scm_check_alg.set(SR_CRYPTOC_HMAC_SHA1);
        sr_crypto_calculate_check_hmac_sha1(
            &kdfinfo2.maskkey,
            key_cells(&mdd_crypto.scr_key),
            &mut check_digest,
        );

        // Copy new encrypted key and HMAC to metadata.
        meta.set_chk_hmac_sha1(&SrCryptoChkHmacSha1 {
            sch_mac: check_digest,
        });

        Ok(()) // Success
    };

    if let Some(pb) = p.take() {
        cells_bzero(pb.cells());
    }

    explicit_bzero(&mut check_digest);
    explicit_bzero(&mut kdfinfo1.maskkey);
    explicit_bzero(&mut kdfinfo2.maskkey);

    rv
}

/// `sr_crypto_create_key_disk`: writes a key disk on `dev` (a `RAID` partition not in use):
/// metadata of a one-chunk `KEYDISK` volume with the volume's UUID, whose optional metadata
/// holds a new random mask key, also kept in `scr_maskkey`. Returns the key disk's chunk.
pub fn sr_crypto_create_key_disk(
    sd: &'static SrDiscipline,
    mdd_crypto: &SrCrypto,
    dev: Dev,
) -> Option<&'static SrChunk> {
    let sc = sd.sd_sc();
    let mut devname = [0u8; 32];

    // Create a metadata structure on the key disk and store keying material in the optional
    // metadata.

    sr_meta_getdevname(sc, dev, &mut devname);
    let dn = Name(devname);

    // Make sure chunk is not already in use.
    let c = sr_chunk_in_use(sc, dev);
    if c != BIOC_SDINVALID && c != BIOC_SDOFFLINE {
        sr_error(sc, format_args!("{} is already in use", dn));
        return None;
    }

    // Open device.
    let vn = key_disk_open(sc, dev, &dn, FREAD | FWRITE)?;

    let key_disk = 'done: {
        // Get partition details.
        if !key_disk_is_raid(sc, vn, dev, &dn) {
            break 'done None;
        }

        // Create and populate chunk metadata.

        let Some(kd) = sr_malloc::<SrChunk>(M_WAITOK) else {
            break 'done None;
        };
        // SAFETY: a zeroed chunk (`SrZeroed`); it becomes the volume's key disk, freed by
        // `sr_crypto_free_resources_internal` (or below on failure).
        let key_disk: &'static SrChunk = unsafe { kd.as_ref() };
        let km = &key_disk.src_meta;

        key_disk.src_dev_mm.set(dev);
        sr_strlcpy_cell(&key_disk.src_devname, &devname);
        key_disk.src_size.set(0);

        let ssdi = sd.sd_meta().ssdi();
        km.scmi().scm_volid.set(ssdi.ssd_level.get());
        km.scmi().scm_chunk_id.set(0);
        km.scmi().scm_size.set(0);
        km.scmi().scm_coerced_size.set(0);
        sr_strlcpy_cell(&km.scmi().scm_devname, &devname);
        km.scmi().scm_uuid.set(ssdi.ssd_uuid.get());

        km.scm_checksum.set(sr_checksum(sc, km.scmi().cells()));

        km.scm_status.set(BIOC_SDONLINE as u32);

        // Create and populate our own discipline and metadata.

        if !key_disk_save(sd, mdd_crypto, key_disk) {
            sr_error(sc, format_args!("could not save metadata to {}", dn));
            // fail:
            sr_free(kd, size_of::<SrChunk>());
            break 'done None;
        }

        Some(key_disk)
    };

    // keep `open = 1' to close dev
    key_disk_close(vn, FREAD | FWRITE);

    key_disk
}

/// The part of `sr_crypto_create_key_disk` that builds the fake one-chunk `KEYDISK`
/// discipline, generates the mask key into its optional metadata and saves it; the fake
/// discipline is freed again. Whether the save succeeded.
fn key_disk_save(
    sd: &'static SrDiscipline,
    mdd_crypto: &SrCrypto,
    key_disk: &'static SrChunk,
) -> bool {
    let Some(smp) = sr_malloc_size::<SrMetadata>(SR_META_BYTES, M_WAITOK) else {
        return false;
    };
    // SAFETY: a zeroed metadata area (`SrZeroed`), freed below.
    let sm: &SrMetadata = unsafe { smp.as_ref() };
    let ssdi = sd.sd_meta().ssdi();
    sm.ssdi().ssd_magic.set(SR_MAGIC);
    sm.ssdi().ssd_version.set(SR_META_VERSION);
    sm.ssd_ondisk.set(0);
    sm.ssdi().ssd_vol_flags.set(0);
    sm.ssdi().ssd_uuid.set(ssdi.ssd_uuid.get());
    sm.ssdi().ssd_chunk_no.set(1);
    sm.ssdi().ssd_volid.set(SR_KEYDISK_VOLID);
    sm.ssdi().ssd_level.set(SR_KEYDISK_LEVEL);
    sm.ssdi().ssd_size.set(0);
    sr_strlcpy_cell(&sm.ssdi().ssd_vendor, b"OPENBSD");
    sr_strlcpy_cell(&sm.ssdi().ssd_product, b"SR KEYDISK");
    let mut rev = [0u8; 4];
    let _ = crate::kern::subr_prf::snprintf(&mut rev, format_args!("{:03}", SR_META_VERSION));
    sm.ssdi().ssd_revision.set(rev);

    let Some(fp) = sr_malloc::<SrDiscipline>(M_WAITOK) else {
        sr_free(smp, SR_META_BYTES);
        return false;
    };
    // SAFETY: a zeroed discipline (`SrZeroed`), freed below; it is never on the softc's
    // list, so nothing else reaches it.
    let fakesd: &'static SrDiscipline = unsafe { fp.as_ref() };
    fakesd.sd_sc.set(sd.sd_sc.get());
    fakesd.sd_meta.set(Some(smp));
    fakesd.sd_meta_type.set(SR_META_F_NATIVE);
    fakesd.sd_vol_status.set(BIOC_SVONLINE);
    sr_strlcpy_cell(&fakesd.sd_name, b"KEYDISK");
    fakesd.sd_meta_opt.init();

    // Add chunk to volume.
    let mut saved = false;
    if fakesd.sd_vol.sv_chunks_alloc(1, M_WAITOK).is_ok() {
        fakesd.sd_vol.set_sv_chunk(0, Some(key_disk));
        fakesd.sd_vol.sv_chunk_list.init();
        // SAFETY: the new chunk is on no list; the fake list goes away below, before the
        // chunk is used anywhere else.
        unsafe { fakesd.sd_vol.sv_chunk_list.insert_head(key_disk) };

        // Generate mask key.
        let mut maskkey = [0u8; SR_CRYPTO_MAXKEYBYTES];
        arc4random_buf(&mut maskkey);
        mdd_crypto.scr_maskkey.set(maskkey);

        // Copy mask key to optional metadata area.
        if let Some(omi) = SrMetaOptItem::alloc(size_of::<SrMetaKeydisk>(), M_WAITOK) {
            let som = omi.omi_som();
            som.som_type.set(SR_OPT_KEYDISK);
            som.som_length.set(size_of::<SrMetaKeydisk>() as u32);
            if let Some(skm) = omi.som_as::<SrMetaKeydisk>() {
                skm.skm_maskkey.set(maskkey);
            }
            // SAFETY: a new item on no list; removed again below.
            unsafe { fakesd.sd_meta_opt.insert_head(omi) };
            let ssdi = sm.ssdi();
            ssdi.ssd_opt_no.set(ssdi.ssd_opt_no.get() + 1);

            // Save metadata.
            saved = sr_meta_save(fakesd, SR_META_DIRTY).is_ok();

            // SAFETY: the item is the fake list's only element (inserted above).
            unsafe { fakesd.sd_meta_opt.remove(omi) };
            cells_bzero(omi.som_cells());
            // SAFETY: from `SrMetaOptItem::alloc`, on no list now, and unused afterwards.
            unsafe { SrMetaOptItem::free(omi) };
        }
        explicit_bzero(&mut maskkey);

        // SAFETY: the chunk is the fake list's only element (inserted above).
        unsafe { fakesd.sd_vol.sv_chunk_list.remove(key_disk) };
        fakesd.sd_vol.sv_chunks_free();
    }

    sr_free(fp, size_of::<SrDiscipline>());
    sr_free(smp, SR_META_BYTES);
    saved
}

/// `bdevvp` and `VOP_OPEN` of a key disk; errors are reported as the C does.
fn key_disk_open(sc: &SrSoftc, dev: Dev, devname: &Name<32>, mode: i32) -> Option<&'static Vnode> {
    let vn = match bdevvp(dev) {
        Ok(Some(vn)) => vn,
        _ => {
            sr_error(sc, format_args!("cannot open key disk {}", devname));
            return None;
        }
    };
    let opened = match curproc() {
        Some(p) => VOP_OPEN(vn, mode, NOCRED, p).is_ok(),
        None => false,
    };
    if !opened {
        // DNPRINTF(SR_D_META, "%s: %s cannot open %s\n", DEVNAME(sc), devname)
        vput(vn);
        return None;
    }
    Some(vn)
}

/// `VOP_CLOSE` and `vput` of a key disk opened by [`key_disk_open`].
fn key_disk_close(vn: &'static Vnode, mode: i32) {
    let _ = VOP_CLOSE(vn, mode, NOCRED, curproc());
    vput(vn);
}

/// Whether the key disk's partition is of type `RAID` (`DIOCGDINFO`).
fn key_disk_is_raid(sc: &SrSoftc, vn: &'static Vnode, dev: Dev, devname: &Name<32>) -> bool {
    let mut label = Disklabel::zeroed();
    let part = diskpart(dev) as usize;
    let Some(p) = curproc() else {
        return false;
    };
    if VOP_IOCTL(vn, DIOCGDINFO, label.as_bytes_mut(), FREAD, NOCRED, p).is_err() {
        // DNPRINTF(SR_D_META, "%s: ioctl failed\n", DEVNAME(sc))
        return false;
    }
    let fstype = label.d_partitions.get(part).map_or(0, |pp| pp.p_fstype);
    if fstype != FS_RAID {
        sr_error(
            sc,
            format_args!("{} partition not of type RAID ({})", devname, fstype),
        );
        return false;
    }
    true
}

/// Where the C reads an original-format key disk's mask key in its `SR_OPT_CRYPTO` item:
/// `omh + sizeof(struct sr_meta_opt_hdr)` on a `struct sr_meta_opt_hdr *` (see the
/// deviations).
const OLD_KEYDISK_MASKKEY_OFFSET: usize = size_of::<SrMetaOptHdr>() * size_of::<SrMetaOptHdr>();

/// `sr_crypto_read_key_disk`: reads the key disk on `dev` and loads its mask key into
/// `scr_maskkey`. Returns the key disk's chunk.
pub fn sr_crypto_read_key_disk(
    sd: &'static SrDiscipline,
    mdd_crypto: &SrCrypto,
    dev: Dev,
) -> Option<&'static SrChunk> {
    let sc = sd.sd_sc();
    let som = SrMetaOptHead::new();
    let mut devname = [0u8; 32];

    // Load a key disk and load keying material into memory.

    som.init();

    sr_meta_getdevname(sc, dev, &mut devname);
    let dn = Name(devname);

    // Make sure chunk is not already in use.
    let c = sr_chunk_in_use(sc, dev);
    if c != BIOC_SDINVALID && c != BIOC_SDOFFLINE {
        sr_error(sc, format_args!("{} is already in use", dn));
        return None;
    }

    // Open device.
    let vn = key_disk_open(sc, dev, &dn, FREAD)?;

    let key_disk = 'done: {
        // Get partition details.
        if !key_disk_is_raid(sc, vn, dev, &dn) {
            break 'done None;
        }

        // Read and validate key disk metadata.
        let Some(smb) = SrMetaBuf::new(SR_META_BYTES, M_WAITOK) else {
            break 'done None;
        };
        let area = smb.cells();
        if sr_meta_native_read(sd, dev, area, ptr::null_mut()).is_err() {
            sr_error(
                sc,
                format_args!("native bootprobe could not read native metadata"),
            );
            break 'done None;
        }

        let sm = smb.md();
        if sr_meta_validate(sd, dev, sm, ptr::null_mut()).is_err() {
            // DNPRINTF(SR_D_META, "%s: invalid metadata\n", DEVNAME(sc))
            break 'done None;
        }

        // Make sure this is a key disk.
        if sm.ssdi().ssd_level.get() != SR_KEYDISK_LEVEL {
            sr_error(sc, format_args!("{} is not a key disk", dn));
            break 'done None;
        }

        // Construct key disk chunk.
        let Some(kd) = sr_malloc::<SrChunk>(M_WAITOK) else {
            break 'done None;
        };
        // SAFETY: a zeroed chunk (`SrZeroed`); it becomes the volume's key disk, freed by
        // `sr_crypto_free_resources_internal`.
        let key_disk: &'static SrChunk = unsafe { kd.as_ref() };
        key_disk.src_dev_mm.set(dev);
        key_disk.src_size.set(0);

        if let Some(cm) = sr_meta_chunk_at(area, 0) {
            key_disk.src_meta.copy_from(cm);
        }

        // Read mask key from optional metadata.
        sr_meta_opt_load(sc, area, &som);
        for omi in som.iter() {
            let omh = omi.omi_som();
            if omh.som_type.get() == SR_OPT_KEYDISK {
                if let Some(skm) = omi.som_as::<SrMetaKeydisk>() {
                    mdd_crypto.scr_maskkey.set(skm.skm_maskkey.get());
                }
            } else if omh.som_type.get() == SR_OPT_CRYPTO {
                // Original keydisk format with key in crypto area.
                let off = OLD_KEYDISK_MASKKEY_OFFSET;
                if let Some(src) = omi.som_cells().get(off..off + SR_CRYPTO_MAXKEYBYTES) {
                    let mut maskkey = [0u8; SR_CRYPTO_MAXKEYBYTES];
                    cells_read(&mut maskkey, src);
                    mdd_crypto.scr_maskkey.set(maskkey);
                    explicit_bzero(&mut maskkey);
                }
            }
        }

        // keep `open = 1' to close dev
        Some(key_disk)
    };

    while let Some(omi) = som.first() {
        // SAFETY: the list's first element; `sr_meta_opt_load` made it with
        // `SrMetaOptItem::alloc`, and it is unused once off the list.
        unsafe {
            som.remove_head();
            SrMetaOptItem::free(omi);
        }
    }

    key_disk_close(vn, FREAD);

    key_disk
}

/// `sr_crypto_free_sessions`: frees the volume's crypto(9) sessions.
pub fn sr_crypto_free_sessions(_sd: &SrDiscipline, mdd_crypto: &SrCrypto) {
    for sid in &mdd_crypto.scr_sid {
        if sid.get() != u64::MAX {
            let _ = crypto_freesession(sid.get());
            sid.set(u64::MAX);
        }
    }
}

/// `sr_crypto_alloc_resources_internal`: the work units with their DMA buffers and crypto
/// requests, the ccbs, the data keys (decrypted with the mask key: `EPERM` when it is
/// wrong) and one crypto(9) session per key in use.
pub fn sr_crypto_alloc_resources_internal(
    sd: &'static SrDiscipline,
    mdd_crypto: &SrCrypto,
) -> Result<(), Errno> {
    // DNPRINTF(SR_D_DIS, "%s: sr_crypto_alloc_resources\n")

    let Some(meta) = mdd_crypto.scr_meta() else {
        sr_error(sd.sd_sc(), format_args!("unknown crypto algorithm"));
        return Err(Errno::EINVAL);
    };
    mdd_crypto.scr_alg.set(CRYPTO_AES_XTS);
    match meta.scm_alg.get() {
        SR_CRYPTOA_AES_XTS_128 => mdd_crypto.scr_klen.set(256),
        SR_CRYPTOA_AES_XTS_256 => mdd_crypto.scr_klen.set(512),
        _ => {
            sr_error(sd.sd_sc(), format_args!("unknown crypto algorithm"));
            return Err(Errno::EINVAL);
        }
    }

    for sid in &mdd_crypto.scr_sid {
        sid.set(u64::MAX);
    }

    if sr_wu_alloc(sd).is_err() {
        sr_error(sd.sd_sc(), format_args!("unable to allocate work units"));
        return Err(Errno::ENOMEM);
    }
    if sr_ccb_alloc(sd).is_err() {
        sr_error(sd.sd_sc(), format_args!("unable to allocate CCBs"));
        return Err(Errno::ENOMEM);
    }
    if sr_crypto_decrypt_key(sd, mdd_crypto).is_err() {
        sr_error(sd.sd_sc(), format_args!("incorrect key or passphrase"));
        return Err(Errno::EPERM);
    }

    // For each work unit allocate the uio, iovec and crypto structures. These have to be
    // allocated now because during runtime we cannot fail an allocation without failing the
    // I/O (which can cause real problems).
    for wu in sd.sd_wu.iter() {
        let crwu = sr_wu_ext::<SrCryptoWuReq>(wu);
        if let Some(p) = malloc(MAXPHYS, M_DEVBUF, M_WAITOK) {
            crwu.cr.cr_dmabuf.set(p.as_ptr());
        }
        let Some(mut crp) = crypto_getreq(SR_CRYPTO_NDESC as i32) else {
            return Err(Errno::ENOMEM);
        };
        crwu.crp_put(core::mem::take(&mut crp.crp_desc));
    }

    // Allocate a session for every 2^SR_CRYPTO_KEY_BLKSHIFT blocks.
    let num_keys = ((sd.sd_meta().ssdi().ssd_size.get() - 1) >> SR_CRYPTO_KEY_BLKSHIFT) + 1;
    if num_keys > SR_CRYPTO_MAXKEYS as i64 {
        return Err(Errno::EFBIG);
    }
    for i in 0..num_keys.max(0) as usize {
        let mut key = mdd_crypto.scr_key[i].get();
        let cri = Cryptoini {
            cri_alg: mdd_crypto.scr_alg.get(),
            cri_klen: mdd_crypto.scr_klen.get(),
            cri_key: &key,
            ..Cryptoini::default()
        };
        let r = crypto_newsession(&cri, 0);
        explicit_bzero(&mut key);
        match r {
            Ok(sid) => mdd_crypto.scr_sid[i].set(sid),
            Err(_) => {
                sr_crypto_free_sessions(sd, mdd_crypto);
                return Err(Errno::EINVAL);
            }
        }
    }

    sr_hotplug_register(sd, sr_crypto_hotplug);

    Ok(())
}

/// `sr_crypto_alloc_resources`.
pub fn sr_crypto_alloc_resources(sd: &'static SrDiscipline) -> Result<(), Errno> {
    sr_crypto_alloc_resources_internal(sd, &sd.mds().mdd_crypto)
}

/// `sr_crypto_free_resources_internal`: the key disk, the sessions, the work units with
/// their buffers and requests, and the ccbs.
pub fn sr_crypto_free_resources_internal(sd: &'static SrDiscipline, mdd_crypto: &SrCrypto) {
    // DNPRINTF(SR_D_DIS, "%s: sr_crypto_free_resources\n")

    if let Some(key_disk) = mdd_crypto.key_disk.take() {
        chunk_bzero(key_disk);
        sr_free(NonNull::from(key_disk), size_of::<SrChunk>());
    }

    sr_hotplug_unregister(sd, sr_crypto_hotplug);

    sr_crypto_free_sessions(sd, mdd_crypto);

    for wu in sd.sd_wu.iter() {
        let crwu = sr_wu_ext::<SrCryptoWuReq>(wu);
        if let Some(p) = NonNull::new(crwu.cr.cr_dmabuf.replace(ptr::null_mut())) {
            free(p, M_DEVBUF, MAXPHYS);
        }
        crwu.crp_free();
    }

    sr_wu_free(sd);
    sr_ccb_free(sd);
}

/// `sr_crypto_free_resources`.
pub fn sr_crypto_free_resources(sd: &'static SrDiscipline) {
    sr_crypto_free_resources_internal(sd, &sd.mds().mdd_crypto);
}

/// `sr_crypto_ioctl_internal`: the discipline ioctls of bioctl(8): `SR_IOCTL_GET_KDFHINT`
/// (copy the KDF hint out) and `SR_IOCTL_CHANGE_PASSPHRASE` (re-mask the keys and save the
/// metadata).
pub fn sr_crypto_ioctl_internal(
    sd: &'static SrDiscipline,
    mdd_crypto: &SrCrypto,
    bd: &mut BiocDiscipline,
) -> Result<(), Errno> {
    let mut kdfpair = [0u8; KDFPAIR_SIZE];
    let mut kdfinfo1 = SrCryptoKdfinfo::default();
    let mut kdfinfo2 = SrCryptoKdfinfo::default();
    let mut raw = [0u8; KDFINFO_SIZE];

    // DNPRINTF(SR_D_IOCTL, "%s: sr_crypto_ioctl %u\n")

    let rv = 'bad: {
        let Some(meta) = mdd_crypto.scr_meta() else {
            break 'bad Err(Errno::EIO);
        };
        match bd.bd_cmd {
            SR_IOCTL_GET_KDFHINT => {
                // Get KDF hint for userland.
                let size = SR_CRYPTO_KDFHINTBYTES;
                if bd.bd_data.is_null() || bd.bd_size as usize > size {
                    break 'bad Err(Errno::EIO);
                }
                let hint = meta.scm_kdfhint.get();
                if copyout(&hint[..bd.bd_size as usize], bd.bd_data as usize).is_err() {
                    break 'bad Err(Errno::EIO);
                }

                Ok(())
            }

            SR_IOCTL_CHANGE_PASSPHRASE => {
                // Attempt to change passphrase.

                let size = KDFPAIR_SIZE;
                if bd.bd_data.is_null() || bd.bd_size as usize > size {
                    break 'bad Err(Errno::EIO);
                }
                if copyin(bd.bd_data as usize, &mut kdfpair).is_err() {
                    break 'bad Err(Errno::EIO);
                }
                let pair = kdfpair_from_bytes(&kdfpair);

                let size = KDFINFO_SIZE;
                if pair.kdfinfo1 == 0 || pair.kdfsize1 as usize > size {
                    break 'bad Err(Errno::EIO);
                }
                if copyin(pair.kdfinfo1, &mut raw).is_err() {
                    break 'bad Err(Errno::EIO);
                }
                kdfinfo1 = kdfinfo_from_bytes(&raw);

                if pair.kdfinfo2 == 0 || pair.kdfsize2 as usize > size {
                    break 'bad Err(Errno::EIO);
                }
                if copyin(pair.kdfinfo2, &mut raw).is_err() {
                    break 'bad Err(Errno::EIO);
                }
                kdfinfo2 = kdfinfo_from_bytes(&raw);

                if sr_crypto_change_maskkey(sd, mdd_crypto, &mut kdfinfo1, &mut kdfinfo2).is_err() {
                    break 'bad Err(Errno::EIO);
                }

                // Save metadata to disk.
                sr_meta_save(sd, SR_META_DIRTY)
            }

            _ => Err(Errno::EIO),
        }
    };

    explicit_bzero(&mut kdfpair);
    explicit_bzero(&mut raw);
    kdfinfo_bzero(&mut kdfinfo1);
    kdfinfo_bzero(&mut kdfinfo2);

    rv
}

/// `sr_crypto_ioctl`.
pub fn sr_crypto_ioctl(sd: &'static SrDiscipline, bd: &mut BiocDiscipline) -> Result<(), Errno> {
    sr_crypto_ioctl_internal(sd, &sd.mds().mdd_crypto, bd)
}

/// `sr_crypto_meta_opt_handler_internal`: takes the `SR_OPT_CRYPTO` item as the volume's
/// crypto metadata; any other is left to the generic handler (`Err`).
pub fn sr_crypto_meta_opt_handler_internal(
    _sd: &SrDiscipline,
    mdd_crypto: &SrCrypto,
    omi: &'static SrMetaOptItem,
) -> Result<(), Errno> {
    if omi.omi_som().som_type.get() == SR_OPT_CRYPTO
        && let Some(meta) = omi.som_as::<SrMetaCrypto>()
    {
        mdd_crypto.scr_meta.set(ptr::from_ref(meta));
        return Ok(());
    }

    Err(Errno::EINVAL)
}

/// `sr_crypto_meta_opt_handler`.
pub fn sr_crypto_meta_opt_handler(
    sd: &'static SrDiscipline,
    omi: &'static SrMetaOptItem,
) -> Result<(), Errno> {
    sr_crypto_meta_opt_handler_internal(sd, &sd.mds().mdd_crypto, omi)
}

/// `sr_crypto_rw`: a write is encrypted into the work unit's buffer, which one ccb then
/// writes; a read is one ccb into the transfer's buffer, decrypted by `sr_crypto_done`.
pub fn sr_crypto_rw(wu: &'static SrWorkunit) -> Result<(), Errno> {
    // DNPRINTF(SR_D_DIS, "%s: sr_crypto_rw wu %p\n")

    sr_validate_io(wu, "sr_crypto_rw")?;

    if wu.xs().flags.get() & SCSI_DATA_OUT != 0 {
        let mdd_crypto = &wu.dis().mds().mdd_crypto;
        let (crwu, rv) = sr_crypto_prepare(wu, mdd_crypto, true, crypto_invoke);

        // DNPRINTF(SR_D_INTR, "%s: sr_crypto_rw: wu %p xs: %p\n")

        if rv.is_err() {
            // fail io (once: see the deviations)
            return Err(Errno::EIO);
        }

        sr_crypto_dev_rw(wu, Some(crwu))
    } else {
        sr_crypto_dev_rw(wu, None)
    }
}

/// `sr_crypto_dev_rw`: the one ccb of the transfer on chunk 0; a write writes `crwu`'s
/// encrypted buffer.
pub fn sr_crypto_dev_rw(
    wu: &'static SrWorkunit,
    crwu: Option<&'static SrCryptoWuReq>,
) -> Result<(), Errno> {
    let sd = wu.dis();
    let xs = wu.xs();

    let blkno = wu.swu_blk_start.get();

    // SAFETY: `xs.data()` is valid and reserved for the transfer until it completes
    // (`ScsiXfer::set_data`), which is after this ccb completes.
    let ccb = unsafe {
        sr_ccb_rw(
            sd,
            0,
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
    if xs.flags.get() & SCSI_DATA_IN == 0 {
        ccb_write_crypted(ccb, crwu);
    }
    sr_wu_enqueue_ccb(wu, ccb);
    sr_schedule_wu(wu);

    Ok(())
}

/// A write ccb writes the work unit's encrypted buffer (`uio->uio_iov->iov_base`, that is
/// `cr_dmabuf`) and carries the crypto work unit in `ccb_opaque`.
pub fn ccb_write_crypted(ccb: &SrCcb, crwu: Option<&'static SrCryptoWuReq>) {
    let Some(crwu) = crwu else {
        panic(format_args!(
            "sr_crypto: write without its crypto work unit"
        ));
    };
    ccb.ccb_buf.b_data.set(crwu.cr_dmabuf());
    ccb.ccb_opaque
        .set(ptr::from_ref(crwu).cast_mut().cast::<c_void>());
}

/// `sr_crypto_done_internal`: a successful read is decrypted in place; then the transfer is
/// completed. RAID 1C's rebuild I/O is left to the rebuild.
pub fn sr_crypto_done_internal(wu: &'static SrWorkunit, mdd_crypto: &SrCrypto) {
    if wu.swu_flags.get() & SR_WUF_REBUILD != 0 {
        // RAID 1C
        return;
    }

    let xs = wu.xs();
    let sd = wu.dis();

    // If this was a successful read, initiate decryption of the data.
    if xs.flags.get() & SCSI_DATA_IN != 0 && xs.error.get() == XS_NOERROR {
        // DNPRINTF(SR_D_INTR, "%s: sr_crypto_done: crypto_invoke %p\n")
        let (_crwu, rv) = sr_crypto_prepare(wu, mdd_crypto, false, crypto_invoke);

        // DNPRINTF(SR_D_INTR, "%s: sr_crypto_done: wu %p xs: %p\n")

        if rv.is_err() {
            xs.error.set(XS_DRIVER_STUFFUP);
        }

        let s = splbio();
        sr_scsi_done(sd, xs);
        splx(s);
        return;
    }

    let s = splbio();
    sr_scsi_done(sd, xs);
    splx(s);
}

/// `sr_crypto_done`.
pub fn sr_crypto_done(wu: &'static SrWorkunit) {
    sr_crypto_done_internal(wu, &wu.dis().mds().mdd_crypto);
}

/// `sr_crypto_hotplug`: disk attach and detach while the volume is up; nothing to do but the
/// debug print.
pub fn sr_crypto_hotplug(_sd: &'static SrDiscipline, _diskp: &Disk, _action: i32) {
    // DNPRINTF(SR_D_MISC, "%s: sr_crypto_hotplug: %s %d\n", DEVNAME(sd->sd_sc),
    //     diskp->dk_name, action)
}

/// The bytes of a key array (`scr_key`, `scm_key`) as one run of `sizeof(scr_key)` cells.
fn key_cells(keys: &[Cell<[u8; SR_CRYPTO_KEYBYTES]>; SR_CRYPTO_MAXKEYS]) -> &[Cell<u8>] {
    // SAFETY: `Cell<[u8; N]>` has the layout of `[u8; N]` (`repr(transparent)` over
    // `UnsafeCell`), so the array is `SR_CRYPTO_KEYSZ` contiguous bytes; `[Cell<u8>]` has
    // the same layout and allows the same writes through a shared reference (the reasoning
    // of `Cell::as_slice_of_cells`), for as long as `keys` is borrowed.
    unsafe { slice::from_raw_parts(keys.as_ptr().cast::<Cell<u8>>(), SR_CRYPTO_KEYSZ) }
}

/// Runs `f` on each 16-byte block of `src` into the same block of `dst` (ECB).
fn ecb_blocks(
    src: &[Cell<u8>],
    dst: &[Cell<u8>],
    f: impl Fn(&[u8; RIJNDAEL128_BLOCK_LEN], &mut [u8; RIJNDAEL128_BLOCK_LEN]),
) {
    let mut a = [0u8; RIJNDAEL128_BLOCK_LEN];
    let mut b = [0u8; RIJNDAEL128_BLOCK_LEN];
    for (s, d) in src
        .as_chunks::<RIJNDAEL128_BLOCK_LEN>()
        .0
        .iter()
        .zip(dst.as_chunks::<RIJNDAEL128_BLOCK_LEN>().0)
    {
        cells_read(&mut a, s);
        f(&a, &mut b);
        cells_write(d, &b);
    }
    explicit_bzero(&mut a);
    explicit_bzero(&mut b);
}

/// `explicit_bzero(&ctx, sizeof(ctx))` of a rijndael context.
fn ctx_bzero(ctx: &mut RijndaelCtx) {
    // SAFETY: `ctx` is exclusively borrowed; a volatile store of the zero context is kept.
    unsafe { ptr::write_volatile(ctx, RijndaelCtx::default()) };
}

/// `explicit_bzero` of cells.
fn cells_bzero(c: &[Cell<u8>]) {
    for b in c {
        // SAFETY: `b.as_ptr()` is valid for a write of its byte, which a cell allows through
        // a shared reference; the store is volatile so that it is kept.
        unsafe { ptr::write_volatile(b.as_ptr(), 0) };
    }
    compiler_fence(Ordering::SeqCst);
}

/// `explicit_bzero` of a key disk chunk before it is freed, through its cells.
fn chunk_bzero(c: &SrChunk) {
    cells_bzero(c.src_meta.cells());
    c.src_dev_mm.set(0);
    c.src_vn.set(None);
    c.src_meta_ondisk.set(0);
    c.src_devname.set([0; 32]);
    c.src_duid.set([0; 8]);
    c.src_size.set(0);
    c.src_secsize.set(0);
    compiler_fence(Ordering::SeqCst);
}

/// `struct sr_crypto_kdfinfo` from its user-space bytes.
fn kdfinfo_from_bytes(b: &[u8; KDFINFO_SIZE]) -> SrCryptoKdfinfo {
    let u32_at = |off: usize| {
        let mut w = [0u8; 4];
        w.copy_from_slice(&b[off..off + 4]);
        u32::from_ne_bytes(w)
    };
    let hint = offset_of!(SrCryptoKdfinfo, _kdfhint);
    let mut k = SrCryptoKdfinfo {
        len: u32_at(offset_of!(SrCryptoKdfinfo, len)),
        flags: u32_at(offset_of!(SrCryptoKdfinfo, flags)),
        ..SrCryptoKdfinfo::default()
    };
    let mk = offset_of!(SrCryptoKdfinfo, maskkey);
    k.maskkey
        .copy_from_slice(&b[mk..mk + SR_CRYPTO_MAXKEYBYTES]);
    k._kdfhint.generic.len = u32_at(hint + offset_of!(SrCryptoGenkdf, len));
    k._kdfhint.generic.r#type = u32_at(hint + offset_of!(SrCryptoGenkdf, r#type));
    k._kdfhint.rounds = u32_at(hint + offset_of!(SrCryptoPbkdf, rounds));
    let salt = hint + offset_of!(SrCryptoPbkdf, salt);
    let salt_len = k._kdfhint.salt.len();
    k._kdfhint.salt.copy_from_slice(&b[salt..salt + salt_len]);
    k
}

/// The user-space bytes of a `struct sr_crypto_kdfinfo` (the inverse of
/// [`kdfinfo_from_bytes`]).
fn kdfinfo_to_bytes(k: &SrCryptoKdfinfo) -> [u8; KDFINFO_SIZE] {
    let mut b = [0u8; KDFINFO_SIZE];
    let mut put = |off: usize, v: &[u8]| b[off..off + v.len()].copy_from_slice(v);
    let hint = offset_of!(SrCryptoKdfinfo, _kdfhint);
    put(offset_of!(SrCryptoKdfinfo, len), &k.len.to_ne_bytes());
    put(offset_of!(SrCryptoKdfinfo, flags), &k.flags.to_ne_bytes());
    put(offset_of!(SrCryptoKdfinfo, maskkey), &k.maskkey);
    put(
        hint + offset_of!(SrCryptoGenkdf, len),
        &k._kdfhint.generic.len.to_ne_bytes(),
    );
    put(
        hint + offset_of!(SrCryptoGenkdf, r#type),
        &k._kdfhint.generic.r#type.to_ne_bytes(),
    );
    put(
        hint + offset_of!(SrCryptoPbkdf, rounds),
        &k._kdfhint.rounds.to_ne_bytes(),
    );
    put(hint + offset_of!(SrCryptoPbkdf, salt), &k._kdfhint.salt);
    b
}

/// `memcpy(scm_kdfhint, &kdfinfo->genkdf, len)` from the user-space bytes of a kdfinfo:
/// at most the hint union is copied (see the deviations).
fn kdfhint_copy(meta: &SrMetaCrypto, raw: &[u8; KDFINFO_SIZE], len: usize) {
    let off = offset_of!(SrCryptoKdfinfo, _kdfhint);
    let n = len.min(KDFHINT_UNION_SIZE).min(SR_CRYPTO_KDFHINTBYTES);
    let mut hint = meta.scm_kdfhint.get();
    hint[..n].copy_from_slice(&raw[off..off + n]);
    meta.scm_kdfhint.set(hint);
    explicit_bzero(&mut hint);
}

/// `explicit_bzero(&kdfinfo, sizeof(kdfinfo))`.
fn kdfinfo_bzero(k: &mut SrCryptoKdfinfo) {
    explicit_bzero(&mut k.maskkey);
    explicit_bzero(&mut k._kdfhint.salt);
    // SAFETY: `k` is exclusively borrowed; a volatile store of the zero value is kept.
    unsafe { ptr::write_volatile(k, SrCryptoKdfinfo::default()) };
}

/// `struct sr_crypto_kdfpair` from its user-space bytes.
fn kdfpair_from_bytes(b: &[u8; KDFPAIR_SIZE]) -> KdfPair {
    let usize_at = |off: usize| {
        let mut w = [0u8; size_of::<usize>()];
        w.copy_from_slice(&b[off..off + size_of::<usize>()]);
        usize::from_ne_bytes(w)
    };
    let u32_at = |off: usize| {
        let mut w = [0u8; 4];
        w.copy_from_slice(&b[off..off + 4]);
        u32::from_ne_bytes(w)
    };
    KdfPair {
        kdfinfo1: usize_at(offset_of!(SrCryptoKdfpair, kdfinfo1)),
        kdfsize1: u32_at(offset_of!(SrCryptoKdfpair, kdfsize1)),
        kdfinfo2: usize_at(offset_of!(SrCryptoKdfpair, kdfinfo2)),
        kdfsize2: u32_at(offset_of!(SrCryptoKdfpair, kdfsize2)),
    }
}

const _: () = {
    // The user structures are copied as the C lays them out.
    assert!(KDFINFO_SIZE == 180);
    assert!(KDFHINT_UNION_SIZE == 140);
    assert!(offset_of!(SrCryptoKdfinfo, _kdfhint) == 40);
    assert!(KDFPAIR_SIZE == 4 * size_of::<usize>());
    // The mask key fits the key schedule (`AES_MAXKEYBYTES`).
    assert!(AES_MAXKEYBYTES >= SR_CRYPTO_MAXKEYBYTES);
    // A request of MAXPHYS bytes fits the work unit's buffer.
    assert!(SR_CRYPTO_DMABUF_SIZE >= MAXPHYS);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Tests of the CRYPTO discipline's key handling (the mask cipher, the HMAC check, key
    // creation, unlocking and re-masking, the user structures) and of its I/O path through
    // crypto(9) and the software driver, against references computed outside this tree
    // (FIPS-197, Python's `hmac`, an IEEE 1619 XTS built on openssl(1)'s AES).

    use super::*;

    extern crate std;
    use std::boxed::Box;
    use std::vec;
    use std::vec::Vec;

    use crate::crypto::crypto::crypto_reset;
    use crate::crypto::cryptosoft::{swcr_init, swcr_reset};
    use crate::crypto::testutil::serial;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::scsi::scsiconf::ScsiXfer;
    use crate::sys::types::Daddr;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn cells(b: &[u8]) -> Vec<Cell<u8>> {
        b.iter().map(|&x| Cell::new(x)).collect()
    }

    fn bytes(c: &[Cell<u8>]) -> Vec<u8> {
        c.iter().map(Cell::get).collect()
    }

    fn sha1(b: &[u8]) -> [u8; SHA1_DIGEST_LENGTH] {
        let mut ctx = Sha1Ctx::new();
        ctx.update(b);
        ctx.finalize()
    }

    /// A zeroed discipline of a zeroed softc, with in-memory metadata (leaked).
    fn discipline() -> &'static SrDiscipline {
        // SAFETY: `SrSoftc` is a `Softc`: all-zero bytes are a valid value of it.
        let sc: &'static SrSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed::<SrSoftc>() }));
        // SAFETY: a zeroed discipline (`SrZeroed`), never freed in the tests.
        let sd: &'static SrDiscipline =
            unsafe { sr_malloc::<SrDiscipline>(M_WAITOK).unwrap().as_ref() };
        sd.sd_sc.set(sc);
        sd.sd_meta.set(Some(
            sr_malloc_size::<SrMetadata>(SR_META_BYTES, M_WAITOK).unwrap(),
        ));
        sd
    }

    /// An optional metadata item of `type_` and `len` bytes.
    fn opt_item(type_: u32, len: usize) -> &'static SrMetaOptItem {
        let omi = SrMetaOptItem::alloc(len, M_WAITOK).unwrap();
        omi.omi_som().som_type.set(type_);
        omi.omi_som().som_length.set(len as u32);
        omi
    }

    /// A CRYPTO discipline whose crypto metadata item is in place (as after assembly).
    fn crypto_volume() -> &'static SrDiscipline {
        let sd = discipline();
        sr_crypto_discipline_init(sd);
        let omi = opt_item(SR_OPT_CRYPTO, size_of::<SrMetaCrypto>());
        sr_crypto_meta_opt_handler(sd, omi).unwrap();
        sd
    }

    fn key_bytes(keys: &[Cell<[u8; SR_CRYPTO_KEYBYTES]>; SR_CRYPTO_MAXKEYS]) -> Vec<u8> {
        bytes(key_cells(keys))
    }

    const MASK_A: [u8; 32] = [0xa5; 32];
    const MASK_B: [u8; 32] = [0x3c; 32];

    #[test]
    fn mask_cipher_is_aes256_ecb() {
        // FIPS-197 appendix C.3
        let key: Vec<u8> = (0..32).collect();
        let p = cells(&hex("00112233445566778899aabbccddeeff"));
        let c = cells(&[0; 16]);
        sr_crypto_encrypt(&p, &c, &key, SR_CRYPTOM_AES_ECB_256).unwrap();
        assert_eq!(bytes(&c), hex("8ea2b7ca516745bfeafc49904b496089"));

        let back = cells(&[0; 16]);
        sr_crypto_decrypt(&c, &back, &key, SR_CRYPTOM_AES_ECB_256).unwrap();
        assert_eq!(bytes(&back), bytes(&p));

        // each 16-byte block on its own (ECB)
        let p2 = cells(&hex("00112233445566778899aabbccddeeff").repeat(2));
        let c2 = cells(&[0; 32]);
        sr_crypto_encrypt(&p2, &c2, &key, SR_CRYPTOM_AES_ECB_256).unwrap();
        assert_eq!(bytes(&c2[16..]), hex("8ea2b7ca516745bfeafc49904b496089"));

        // the C's -1
        assert_eq!(sr_crypto_encrypt(&p, &c, &key, 7), Err(Errno::EINVAL));
        assert_eq!(sr_crypto_decrypt(&c, &p, &key, 0), Err(Errno::EINVAL));
    }

    #[test]
    fn check_is_hmac_sha1_keyed_with_sha1_of_the_mask_key() {
        // hmac.new(sha1(bytes(range(32))).digest(), key, sha1) in Python
        let maskkey: Vec<u8> = (0..32).collect();
        let key: Vec<u8> = (0..SR_CRYPTO_KEYSZ).map(|i| (i * 7 + 3) as u8).collect();
        let mut digest = [0u8; SHA1_DIGEST_LENGTH];
        sr_crypto_calculate_check_hmac_sha1(&maskkey, &cells(&key), &mut digest);
        assert_eq!(
            digest.to_vec(),
            hex("f6bb212fe6c7735143942ccd3448af16c3a324c0")
        );
    }

    #[test]
    fn created_keys_unlock_with_the_mask_key_only() {
        let _g = setup_real_memory();
        let sd = crypto_volume();
        let mdd = &sd.mds().mdd_crypto;
        let meta = mdd.scr_meta().unwrap();

        mdd.scr_maskkey.set(MASK_A);
        sr_crypto_create_keys(sd, mdd).unwrap();
        assert_eq!(meta.scm_alg.get(), SR_CRYPTOA_AES_XTS_256);
        assert_eq!(meta.scm_mask_alg.get(), SR_CRYPTOM_AES_ECB_256);
        assert_eq!(meta.scm_check_alg.get(), SR_CRYPTOC_HMAC_SHA1);
        assert_eq!(meta.scm_flags.get(), SR_CRYPTOF_KEY | SR_CRYPTOF_KDFHINT);
        // the plain keys are gone; the masked ones are on the metadata
        assert!(key_bytes(&mdd.scr_key).iter().all(|&b| b == 0));
        assert!(key_bytes(&meta.scm_key).iter().any(|&b| b != 0));

        // unlock: the keys decrypt, check, and the mask key is forgotten
        sr_crypto_decrypt_key(sd, mdd).unwrap();
        assert_eq!(mdd.scr_maskkey.get(), [0; 32]);
        let plain = key_bytes(&mdd.scr_key);
        assert!(plain.iter().any(|&b| b != 0));
        // they are the masked keys under AES-256-ECB
        let again = cells(&[0; SR_CRYPTO_KEYSZ]);
        sr_crypto_encrypt(&cells(&plain), &again, &MASK_A, SR_CRYPTOM_AES_ECB_256).unwrap();
        assert_eq!(bytes(&again), key_bytes(&meta.scm_key));
        // and the stored check is their HMAC
        let mut digest = [0u8; SHA1_DIGEST_LENGTH];
        sr_crypto_calculate_check_hmac_sha1(&MASK_A, &cells(&plain), &mut digest);
        assert_eq!(meta.chk_hmac_sha1().sch_mac, digest);

        // a wrong mask key fails the check and leaves no key behind
        mdd.scr_maskkey.set(MASK_B);
        assert_eq!(sr_crypto_decrypt_key(sd, mdd), Err(Errno::EIO));
        assert!(key_bytes(&mdd.scr_key).iter().all(|&b| b == 0));
        assert_eq!(mdd.scr_maskkey.get(), [0; 32]);

        // the check algorithm must be known
        meta.scm_check_alg.set(0);
        mdd.scr_maskkey.set(MASK_A);
        assert_eq!(sr_crypto_decrypt_key(sd, mdd), Err(Errno::EIO));
    }

    #[test]
    fn change_maskkey_rewraps_the_same_keys() {
        let _g = setup_real_memory();
        let sd = crypto_volume();
        let mdd = &sd.mds().mdd_crypto;
        let meta = mdd.scr_meta().unwrap();

        mdd.scr_maskkey.set(MASK_A);
        sr_crypto_create_keys(sd, mdd).unwrap();
        mdd.scr_maskkey.set(MASK_A);
        sr_crypto_decrypt_key(sd, mdd).unwrap();
        let plain = key_bytes(&mdd.scr_key);

        let mut k1 = SrCryptoKdfinfo {
            maskkey: MASK_A,
            ..SrCryptoKdfinfo::default()
        };
        let mut k2 = SrCryptoKdfinfo {
            flags: SR_CRYPTOKDF_KEY | SR_CRYPTOKDF_HINT,
            maskkey: MASK_B,
            ..SrCryptoKdfinfo::default()
        };
        k2._kdfhint.generic.len = size_of::<SrCryptoPbkdf>() as u32;
        k2._kdfhint.generic.r#type = SR_CRYPTOKDFT_BCRYPT_PBKDF;
        k2._kdfhint.rounds = 16;
        k2._kdfhint.salt = [0x77; 128];
        let hint = kdfinfo_to_bytes(&k2)[40..180].to_vec();
        sr_crypto_change_maskkey(sd, mdd, &mut k1, &mut k2).unwrap();
        assert_eq!(k1.maskkey, [0; 32]);
        assert_eq!(k2.maskkey, [0; 32]);
        let kdfhint = meta.scm_kdfhint.get();
        assert_eq!(&kdfhint[..140], &hint[..]);
        assert!(kdfhint[140..].iter().all(|&b| b == 0));

        // the new mask key unlocks the same keys, the old one no more
        mdd.scr_maskkey.set(MASK_B);
        sr_crypto_decrypt_key(sd, mdd).unwrap();
        assert_eq!(key_bytes(&mdd.scr_key), plain);
        mdd.scr_maskkey.set(MASK_A);
        assert_eq!(sr_crypto_decrypt_key(sd, mdd), Err(Errno::EIO));

        // a wrong old mask key is EPERM and changes nothing
        mdd.scr_maskkey.set(MASK_B);
        sr_crypto_decrypt_key(sd, mdd).unwrap();
        let masked = key_bytes(&meta.scm_key);
        let mut k1 = SrCryptoKdfinfo {
            maskkey: MASK_A,
            ..SrCryptoKdfinfo::default()
        };
        let mut k2 = SrCryptoKdfinfo {
            maskkey: [1; 32],
            ..SrCryptoKdfinfo::default()
        };
        assert_eq!(
            sr_crypto_change_maskkey(sd, mdd, &mut k1, &mut k2),
            Err(Errno::EPERM)
        );
        assert_eq!(key_bytes(&meta.scm_key), masked);
        assert_eq!((k1.maskkey, k2.maskkey), ([0; 32], [0; 32]));
    }

    #[test]
    fn kdfinfo_bytes_round_trip() {
        let raw: [u8; KDFINFO_SIZE] = core::array::from_fn(|i| (i * 31 + 5) as u8);
        let k = kdfinfo_from_bytes(&raw);
        assert_eq!(k.len, u32::from_ne_bytes([raw[0], raw[1], raw[2], raw[3]]));
        assert_eq!(&k.maskkey[..], &raw[8..40]);
        assert_eq!(k._kdfhint.salt[127], raw[179]);
        assert_eq!(kdfinfo_to_bytes(&k), raw);

        let mut k = k;
        kdfinfo_bzero(&mut k);
        assert_eq!(kdfinfo_to_bytes(&k), [0; KDFINFO_SIZE]);

        let mut pair = [0u8; KDFPAIR_SIZE];
        pair[..8].copy_from_slice(&0x1000usize.to_ne_bytes());
        pair[8..12].copy_from_slice(&180u32.to_ne_bytes());
        pair[16..24].copy_from_slice(&0x2000usize.to_ne_bytes());
        pair[24..28].copy_from_slice(&176u32.to_ne_bytes());
        let p = kdfpair_from_bytes(&pair);
        assert_eq!(
            (p.kdfinfo1, p.kdfsize1, p.kdfinfo2, p.kdfsize2),
            (0x1000, 180, 0x2000, 176)
        );
    }

    /// A `bioc_createraid` handing `kdf` in (`BIOC_SOIN`).
    fn createraid_in(raw: &mut [u8; KDFINFO_SIZE]) -> BiocCreateraid {
        // SAFETY: a plain C structure of integers and null pointers: all-zero bytes are a value.
        let mut bc: BiocCreateraid = unsafe { core::mem::zeroed() };
        bc.bc_key_disk = NODEV;
        bc.bc_opaque_flags = BIOC_SOIN;
        bc.bc_opaque_size = KDFINFO_SIZE as u32;
        bc.bc_opaque = raw.as_mut_ptr().cast();
        bc
    }

    #[test]
    fn get_kdf_takes_the_hint_and_the_mask_key() {
        let _g = setup_real_memory();
        let sd = crypto_volume();
        let mdd = &sd.mds().mdd_crypto;

        let mut k = SrCryptoKdfinfo {
            len: KDFINFO_SIZE as u32,
            flags: SR_CRYPTOKDF_KEY | SR_CRYPTOKDF_HINT,
            maskkey: MASK_B,
            ..SrCryptoKdfinfo::default()
        };
        k._kdfhint.generic.len = 8;
        k._kdfhint.generic.r#type = SR_CRYPTOKDFT_KEYDISK;
        let mut raw = kdfinfo_to_bytes(&k);
        let mut bc = createraid_in(&mut raw);
        sr_crypto_get_kdf(&mut bc, sd, mdd).unwrap();
        assert_eq!(bc.bc_opaque_status, BIOC_SOINOUT_OK);
        assert_eq!(mdd.scr_maskkey.get(), MASK_B);
        assert_eq!(
            &mdd.scr_meta().unwrap().scm_kdfhint.get()[..8],
            &raw[40..48]
        );

        // the length inside must match, and the size, and the direction
        k.len = 4;
        let mut raw = kdfinfo_to_bytes(&k);
        let mut bc = createraid_in(&mut raw);
        assert_eq!(sr_crypto_get_kdf(&mut bc, sd, mdd), Err(Errno::EINVAL));
        bc.bc_opaque_size = 4;
        assert_eq!(sr_crypto_get_kdf(&mut bc, sd, mdd), Err(Errno::EINVAL));
        bc.bc_opaque_flags = BIOC_SOOUT;
        assert_eq!(sr_crypto_get_kdf(&mut bc, sd, mdd), Err(Errno::EINVAL));
    }

    #[test]
    fn meta_create_makes_a_passphrase_volume() {
        let _g = setup_real_memory();
        let sd = discipline();
        sr_crypto_discipline_init(sd);
        sd.sd_meta().ssdi().ssd_size.set(2048);
        let mdd = &sd.mds().mdd_crypto;

        let k = SrCryptoKdfinfo {
            len: KDFINFO_SIZE as u32,
            flags: SR_CRYPTOKDF_KEY,
            maskkey: MASK_A,
            ..SrCryptoKdfinfo::default()
        };
        let mut raw = kdfinfo_to_bytes(&k);

        // bioctl first asks for a hint: none yet
        let mut bc = createraid_in(&mut raw);
        bc.bc_opaque_flags = BIOC_SOOUT;
        assert_eq!(sr_crypto_meta_create(sd, mdd, &mut bc), Err(Errno::EAGAIN));
        assert_eq!(bc.bc_opaque_status, BIOC_SOINOUT_FAILED);

        // a passphrase volume must not be auto-assembled
        let sd = discipline();
        sr_crypto_discipline_init(sd);
        let mdd = &sd.mds().mdd_crypto;
        let mut bc = createraid_in(&mut raw);
        bc.bc_flags = BIOC_SCNOAUTOASSEMBLE;
        sr_crypto_meta_create(sd, mdd, &mut bc).unwrap();
        assert_eq!(sd.sd_meta().ssdi().ssd_opt_no.get(), 1);
        let omi = sd.sd_meta_opt.first().unwrap();
        assert_eq!(omi.omi_som().som_type.get(), SR_OPT_CRYPTO);
        let meta = mdd.scr_meta().unwrap();
        assert!(core::ptr::eq(meta, omi.som_as::<SrMetaCrypto>().unwrap()));
        assert_eq!(meta.scm_flags.get(), SR_CRYPTOF_KEY | SR_CRYPTOF_KDFHINT);
        mdd.scr_maskkey.set(MASK_A);
        sr_crypto_decrypt_key(sd, mdd).unwrap();
    }

    #[test]
    fn set_key_paths() {
        let _g = setup_real_memory();
        let sd = crypto_volume();
        let mdd = &sd.mds().mdd_crypto;
        let mut hint = [0u8; SR_CRYPTO_KDFHINTBYTES];
        hint[..4].copy_from_slice(&[1, 2, 3, 4]);
        mdd.scr_meta().unwrap().scm_kdfhint.set(hint);
        let mut raw = [0u8; KDFINFO_SIZE];

        // the boot loader's key
        let mut bc = createraid_in(&mut raw);
        sr_crypto_set_key(sd, mdd, &mut bc, 1, Some(&MASK_B)).unwrap();
        assert_eq!(mdd.scr_maskkey.get(), MASK_B);
        assert_eq!(
            sr_crypto_set_key(sd, mdd, &mut bc, 1, Some(&MASK_B[..8])),
            Err(Errno::EINVAL)
        );

        // the hint goes out first
        let mut out = [0u8; 16];
        bc.bc_opaque_flags = BIOC_SOOUT;
        bc.bc_opaque = out.as_mut_ptr().cast();
        bc.bc_opaque_size = 16;
        assert_eq!(
            sr_crypto_set_key(sd, mdd, &mut bc, 1, None),
            Err(Errno::EAGAIN)
        );
        assert_eq!(bc.bc_opaque_status, BIOC_SOINOUT_OK);
        assert_eq!(&out[..4], &[1, 2, 3, 4]);
        bc.bc_opaque_size = 300;
        assert_eq!(
            sr_crypto_set_key(sd, mdd, &mut bc, 1, None),
            Err(Errno::EINVAL)
        );

        // nothing to go by
        bc.bc_opaque_flags = 0;
        assert_eq!(
            sr_crypto_set_key(sd, mdd, &mut bc, 1, None),
            Err(Errno::EINVAL)
        );
    }

    #[test]
    fn get_kdfhint_ioctl() {
        let _g = setup_real_memory();
        let sd = crypto_volume();
        let mut hint = [0u8; SR_CRYPTO_KDFHINTBYTES];
        hint[250] = 9;
        sd.mds()
            .mdd_crypto
            .scr_meta()
            .unwrap()
            .scm_kdfhint
            .set(hint);

        let mut out = [0u8; SR_CRYPTO_KDFHINTBYTES];
        // SAFETY: a plain C structure of integers and null pointers: all-zero bytes are a value.
        let mut bd: BiocDiscipline = unsafe { core::mem::zeroed() };
        bd.bd_cmd = SR_IOCTL_GET_KDFHINT;
        bd.bd_size = SR_CRYPTO_KDFHINTBYTES as u32;
        bd.bd_data = out.as_mut_ptr().cast();
        sr_crypto_ioctl(sd, &mut bd).unwrap();
        assert_eq!(out, hint);

        bd.bd_size += 1;
        assert_eq!(sr_crypto_ioctl(sd, &mut bd), Err(Errno::EIO));
        bd.bd_cmd = 0x77;
        assert_eq!(sr_crypto_ioctl(sd, &mut bd), Err(Errno::EIO));
    }

    #[test]
    fn meta_opt_handler_takes_only_the_crypto_item() {
        let _g = setup_real_memory();
        let sd = discipline();
        sr_crypto_discipline_init(sd);
        let mdd = &sd.mds().mdd_crypto;
        let kd = opt_item(SR_OPT_KEYDISK, size_of::<SrMetaKeydisk>());
        assert_eq!(sr_crypto_meta_opt_handler(sd, kd), Err(Errno::EINVAL));
        assert!(mdd.scr_meta().is_none());
        // too short to be one
        let short = opt_item(SR_OPT_CRYPTO, 64);
        assert_eq!(sr_crypto_meta_opt_handler(sd, short), Err(Errno::EINVAL));
        let cr = opt_item(SR_OPT_CRYPTO, size_of::<SrMetaCrypto>());
        sr_crypto_meta_opt_handler(sd, cr).unwrap();
        assert!(core::ptr::eq(
            mdd.scr_meta().unwrap(),
            cr.som_as::<SrMetaCrypto>().unwrap()
        ));
    }

    #[test]
    fn discipline_init_sets_up_crypto() {
        let _g = setup_real_memory();
        let sd = discipline();
        sr_crypto_discipline_init(sd);
        assert_eq!(sd.sd_type.get(), SR_MD_CRYPTO);
        assert_eq!(&sd.sd_name.get()[..7], b"CRYPTO\0");
        assert_eq!(sd.sd_max_wu.get(), SR_CRYPTO_NOWU);
        assert_eq!(sd.sd_wu_size(), size_of::<SrCryptoWuReq>());
        assert!(
            sd.mds()
                .mdd_crypto
                .scr_sid
                .iter()
                .all(|s| s.get() == u64::MAX)
        );
        assert!(sd.sd_scsi_rw.get().is_some() && sd.sd_scsi_done.get().is_some());
        assert!(sd.sd_scsi_wu_done.get().is_none());
    }

    /// The XTS key of the reference (`bytes(i * 3 + 1)`), its plain text (`bytes(i * 13 + 7)`)
    /// over two sectors, and the reference ciphertext of those two sectors at blocks 5 and 6
    /// (`d3-tools/xts.py`: IEEE 1619 with openssl's AES-256-ECB): its SHA1 and the first 16
    /// bytes of each sector.
    const XTS_SHA1: &str = "9cada410012f6ea3ce8a2e1fe999ca41b84ba6af";
    const XTS_SECTOR0: &str = "ad119307df6b51ba05c466c1c852bc37";
    const XTS_SECTOR1: &str = "4a526b9a9942f899614c13628e586b2c";

    fn xts_plain() -> Vec<u8> {
        (0..1024).map(|i| (i * 13 + 7) as u8).collect()
    }

    /// A volume with the reference key as data key 0 and its software crypto session, and a
    /// work unit (leaked) with its buffer and descriptors as `sr_crypto_alloc_resources`
    /// leaves it.
    fn xts_volume() -> (&'static SrDiscipline, &'static SrCryptoWuReq) {
        let sd = discipline();
        sr_crypto_discipline_init(sd);
        let mdd = &sd.mds().mdd_crypto;
        mdd.scr_alg.set(CRYPTO_AES_XTS);
        mdd.scr_klen.set(512);
        let key: [u8; 64] = core::array::from_fn(|i| (i * 3 + 1) as u8);
        mdd.scr_key[0].set(key);
        let cri = Cryptoini {
            cri_alg: CRYPTO_AES_XTS,
            cri_klen: 512,
            cri_key: &key,
            ..Cryptoini::default()
        };
        mdd.scr_sid[0].set(crypto_newsession(&cri, 0).unwrap());

        // SAFETY: a zeroed crypto work unit (`SrZeroed`), leaked.
        let crwu: &'static SrCryptoWuReq =
            unsafe { sr_malloc::<SrCryptoWuReq>(M_WAITOK).unwrap().as_ref() };
        crwu.cr.cr_wu.swu_dis.set(sd);
        crwu.cr
            .cr_dmabuf
            .set(malloc(MAXPHYS, M_DEVBUF, M_WAITOK).unwrap().as_ptr());
        let mut crp = crypto_getreq(SR_CRYPTO_NDESC as i32).unwrap();
        crwu.crp_put(core::mem::take(&mut crp.crp_desc));
        (sd, crwu)
    }

    #[test]
    fn sectors_are_aes_xts_with_the_block_number_as_tweak() {
        let _g = setup_real_memory();
        let _s = serial();
        crypto_reset();
        swcr_reset();
        swcr_init();
        let (sd, crwu) = xts_volume();
        let mdd = &sd.mds().mdd_crypto;

        let plain = xts_plain();
        let mut buf = plain.clone();
        sr_crypto_request(crwu, mdd, &mut buf, 5, true, crypto_invoke)
            .unwrap()
            .unwrap();
        assert_eq!(sha1(&buf).to_vec(), hex(XTS_SHA1));
        assert_eq!(buf[..16].to_vec(), hex(XTS_SECTOR0));
        assert_eq!(buf[512..528].to_vec(), hex(XTS_SECTOR1));

        // the same plain text at another block is another ciphertext
        let mut other = plain.clone();
        sr_crypto_request(crwu, mdd, &mut other, 6, true, crypto_invoke)
            .unwrap()
            .unwrap();
        assert_ne!(other, buf);

        sr_crypto_request(crwu, mdd, &mut buf, 5, false, crypto_invoke)
            .unwrap()
            .unwrap();
        assert_eq!(buf, plain);

        // the descriptors were given back, empty, their allocation kept
        let descs: Vec<Cryptodesc<'_>> = crwu.crp_take().unwrap();
        assert!(descs.is_empty() && descs.capacity() >= SR_CRYPTO_NDESC);
        crwu.crp_put(descs);

        // the request looks as the C's does
        let mut seen = Vec::new();
        sr_crypto_request(crwu, mdd, &mut buf, 0x1234, true, |crp| {
            seen.push((crp.crp_ndesc, crp.crp_ilen, crp.crp_flags, crp.crp_sid));
            for (i, d) in crp.crp_desc.iter().enumerate() {
                let mut iv = [0u8; 8];
                iv.copy_from_slice(&d.crd_iv()[..8]);
                assert_eq!(Daddr::from_ne_bytes(iv), 0x1234 + i as Daddr);
                assert_eq!(d.crd_skip, (i * DEV_BSIZE) as i32);
                assert_eq!(d.crd_len, DEV_BSIZE as i32);
                assert_eq!(
                    d.crd_flags,
                    CRD_F_ENCRYPT | CRD_F_IV_PRESENT | CRD_F_IV_EXPLICIT
                );
                assert_eq!(d.CRD_INI.cri_alg, CRYPTO_AES_XTS);
                assert_eq!(d.CRD_INI.cri_klen, 512);
                assert_eq!(d.CRD_INI.cri_key.len(), SR_CRYPTO_KEYBYTES);
            }
            Ok(())
        })
        .unwrap()
        .unwrap();
        assert_eq!(seen, vec![(2, 1024, CRYPTO_F_IOV, mdd.scr_sid[0].get())]);

        // a block past the first 2^30 uses the next key's session (none here)
        let r = sr_crypto_request(
            crwu,
            mdd,
            &mut buf,
            1 << SR_CRYPTO_KEY_BLKSHIFT,
            true,
            crypto_invoke,
        )
        .unwrap();
        assert!(r.is_err());

        // a work unit without descriptors cannot run a request
        let none = crwu.crp_take().unwrap();
        assert!(sr_crypto_request(crwu, mdd, &mut buf, 5, true, crypto_invoke).is_none());
        crwu.crp_put(none);
    }

    #[test]
    fn prepare_encrypts_writes_aside_and_decrypts_reads_in_place() {
        let _g = setup_real_memory();
        let _s = serial();
        crypto_reset();
        swcr_reset();
        swcr_init();
        let (sd, crwu) = xts_volume();
        let mdd = &sd.mds().mdd_crypto;
        let wu = &crwu.cr.cr_wu;
        wu.swu_blk_start.set(5);
        let xs: &'static ScsiXfer = Box::leak(Box::new(ScsiXfer::new()));
        wu.swu_xs.set(Some(xs));

        // a write: the transfer's data is left alone, the work unit's buffer is encrypted
        let plain = xts_plain();
        let data: &'static mut [u8] = plain.clone().leak();
        xs.flags.set(SCSI_DATA_OUT);
        // SAFETY: a leaked buffer of 1024 bytes that only this transfer uses.
        unsafe { xs.set_data(data.as_mut_ptr(), 1024) };
        let (got, rv) = sr_crypto_prepare(wu, mdd, true, crypto_invoke);
        rv.unwrap();
        assert!(core::ptr::eq(got, crwu));
        // SAFETY: the work unit's MAXPHYS buffer, not in use now.
        let dma = unsafe { slice::from_raw_parts(crwu.cr_dmabuf(), 1024) };
        assert_eq!(sha1(dma).to_vec(), hex(XTS_SHA1));
        // SAFETY: the test owns the transfer.
        assert_eq!(unsafe { xs.data_slice() }.to_vec(), plain);

        // the write's ccb writes the encrypted buffer
        let ccb: &'static SrCcb = Box::leak(Box::new(SrCcb::new()));
        ccb_write_crypted(ccb, Some(crwu));
        assert_eq!(ccb.ccb_buf.b_data.get(), crwu.cr_dmabuf());
        assert_eq!(
            ccb.ccb_opaque.get(),
            ptr::from_ref(crwu).cast_mut().cast::<c_void>()
        );

        // a read of what was written is decrypted in place
        let back: &'static mut [u8] = dma.to_vec().leak();
        xs.flags.set(SCSI_DATA_IN);
        // SAFETY: as above.
        unsafe { xs.set_data(back.as_mut_ptr(), 1024) };
        let (_, rv) = sr_crypto_prepare(wu, mdd, false, crypto_invoke);
        rv.unwrap();
        // SAFETY: the test owns the transfer.
        assert_eq!(unsafe { xs.data_slice() }.to_vec(), plain);

        // a rebuild's work unit is not touched (RAID 1C)
        wu.swu_flags.set(SR_WUF_REBUILD);
        xs.error.set(XS_NOERROR);
        sr_crypto_done_internal(wu, mdd);
        // SAFETY: the test owns the transfer.
        assert_eq!(unsafe { xs.data_slice() }.to_vec(), plain);
    }

    #[test]
    fn free_sessions_forgets_them() {
        let _g = setup_real_memory();
        let _s = serial();
        crypto_reset();
        swcr_reset();
        swcr_init();
        let (sd, _crwu) = xts_volume();
        let mdd = &sd.mds().mdd_crypto;
        assert_ne!(mdd.scr_sid[0].get(), u64::MAX);
        sr_crypto_free_sessions(sd, mdd);
        assert!(mdd.scr_sid.iter().all(|s| s.get() == u64::MAX));
    }

    #[test]
    fn old_keydisk_offset_is_the_cs_pointer_arithmetic() {
        // `omh + sizeof(struct sr_meta_opt_hdr)` with `omh` a `struct sr_meta_opt_hdr *`
        assert_eq!(size_of::<SrMetaOptHdr>(), 24);
        assert_eq!(OLD_KEYDISK_MASKKEY_OFFSET, 576);
        assert!(OLD_KEYDISK_MASKKEY_OFFSET + SR_CRYPTO_MAXKEYBYTES <= size_of::<SrMetaCrypto>());
    }
}
/* </TESTS> */
