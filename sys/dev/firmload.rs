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
 * Copyright (c) 2004 Theo de Raadt <deraadt@openbsd.org>
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
//! `dev/firmload.c`: `loadfirmware(9)`, which reads a firmware file for a driver from
//! `/etc/firmware`.
//!
//! Upstream: sys/dev/firmload.c @ 3ce1f3f79392
//!
//! The file is looked up as the kernel (`KERNELPATH`, `NOFOLLOW`) under the root file
//! system, must be a regular file of at least one byte and at most [`FIRMWARE_MAX`], and is
//! read whole into a `M_DEVBUF` buffer the caller frees. Without a root vnode in use the
//! call fails with `EIO`, which is what a driver that attaches before the root is mounted
//! sees (its `fxp_init` runs later, when the interface is configured).
//!
//! ## Deviations
//! - The C returns the buffer and its length through two out-parameters and an errno; here
//!   `loadfirmware` returns `Result<(NonNull<u8>, usize), Errno>`. The caller frees the
//!   buffer with `free(buf, M_DEVBUF, len)`.
//! - `RAMDISK_HOOKS` (bsd.rd's `option`; the retry under `/mnt/etc/firmware` for a ramdisk
//!   that has mounted the disk being installed) is not configured in this kernel, as
//!   everywhere else (`vga_pci`): the `#ifdef RAMDISK_HOOKS` block is not compiled.
//! - The path is built in the `M_TEMP` buffer the C allocates, NUL-terminated, and handed to
//!   `namei` as a slice that ends at its first NUL.
//! - With no current process (`curproc()` is `None`, as in the host tests) the call fails
//!   with `EIO`; the C takes `curproc` unchecked and hands it to `NDINIT` and `VOP_GETATTR`,
//!   which would dereference a null pointer there. On a running kernel a process is always
//!   current, so no driver sees the difference.

use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::kern::init_main::rootvp;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::vfs_lookup::{namei, ndinit};
use crate::kern::vfs_subr::{vcount, vput};
use crate::kern::vfs_vops::{VOP_GETATTR, VOP_READ};
use crate::machine::cpu::curproc;
use crate::sys::device::FIRMWARE_MAX;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_TEMP};
use crate::sys::namei::{KERNELPATH, LOCKLEAF, LOOKUP, NOFOLLOW, NiDirp};
use crate::sys::param::MAXPATHLEN;
use crate::sys::pledge::PLEDGE_RPATH;
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::vnode::{VREG, Vattr};

/// The directory firmware files live in.
const FIRMWARE_DIR: &str = "/etc/firmware/";

/// `loadfirmware(name, &buf, &buflen)`: reads `/etc/firmware/<name>`. The buffer is `M_DEVBUF`
/// memory of the returned length; the caller owns it.
pub fn loadfirmware(name: &str) -> Result<(NonNull<u8>, usize), Errno> {
    let Some(p) = curproc() else {
        return Err(Errno::EIO);
    };

    match rootvp() {
        Some(rv) if vcount(rv) != 0 => {}
        _ => return Err(Errno::EIO),
    }

    let Some(pathbuf) = malloc(MAXPATHLEN, M_TEMP, M_NOWAIT) else {
        return Err(Errno::ENOMEM);
    };
    // SAFETY: a fresh `MAXPATHLEN`-byte `M_TEMP` allocation nothing else refers to, freed
    // below after the last use of this slice.
    let path = unsafe { core::slice::from_raw_parts_mut(pathbuf.as_ptr(), MAXPATHLEN) };

    let error = loadfirmware_path(p, path, name);

    free(pathbuf, M_TEMP, MAXPATHLEN);
    error
}

/// The part of `loadfirmware` that runs with the path buffer: `snprintf`, the lookup, the
/// checks and the read.
fn loadfirmware_path(
    p: &'static crate::sys::proc::Proc,
    path: &mut [u8],
    name: &str,
) -> Result<(NonNull<u8>, usize), Errno> {
    let full = FIRMWARE_DIR.len() + name.len();
    // snprintf(path, MAXPATHLEN, "/etc/firmware/%s", name) >= MAXPATHLEN
    if full >= MAXPATHLEN {
        return Err(Errno::ENAMETOOLONG);
    }
    path[..FIRMWARE_DIR.len()].copy_from_slice(FIRMWARE_DIR.as_bytes());
    path[FIRMWARE_DIR.len()..full].copy_from_slice(name.as_bytes());
    path[full] = 0;

    let mut nid = ndinit(
        LOOKUP,
        NOFOLLOW | LOCKLEAF | KERNELPATH,
        NiDirp::Sys(&path[..=full]),
        p,
    );
    nid.ni_pledge = PLEDGE_RPATH;
    namei(&mut nid)?;
    let Some(vp) = nid.ni_vp else {
        return Err(Errno::ENOENT);
    };

    let cred = ptr::from_ref(p.ucred());
    let mut va = Vattr::new();
    if let Err(e) = VOP_GETATTR(vp, &mut va, cred, p) {
        vput(vp);
        return Err(e);
    }
    if vp.v_type.get() != VREG || va.va_size == 0 {
        vput(vp);
        return Err(Errno::EINVAL);
    }
    let size = va.va_size as usize;
    if size > FIRMWARE_MAX {
        vput(vp);
        return Err(Errno::E2BIG);
    }
    let Some(buf) = malloc(size, M_DEVBUF, M_NOWAIT) else {
        vput(vp);
        return Err(Errno::ENOMEM);
    };

    let mut iov = [Iovec {
        iov_base: buf.as_ptr().cast::<c_void>(),
        iov_len: size,
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: size,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: Some(p),
    };

    let result = match VOP_READ(vp, &mut uio, 0, cred) {
        Ok(()) => Ok((buf, size)),
        Err(e) => {
            free(buf, M_DEVBUF, size);
            Err(e)
        }
    };

    vput(vp);
    result
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_mounted_root_the_call_fails_with_eio() {
        // With no root vnode (or no current process) the call fails before any lookup.
        if rootvp().is_none() {
            assert_eq!(loadfirmware("fxp-d101s").err(), Some(Errno::EIO));
        }
    }

    #[test]
    fn a_name_that_does_not_fit_is_enametoolong() {
        let mut path = [0u8; MAXPATHLEN];
        let name = "x".repeat(MAXPATHLEN);
        let Some(p) = curproc() else {
            return;
        };
        assert_eq!(
            loadfirmware_path(p, &mut path, &name).err(),
            Some(Errno::ENAMETOOLONG)
        );
    }
}
/* </TESTS> */
