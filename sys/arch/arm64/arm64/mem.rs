/*	$OpenBSD: mem.c,v 1.10 2024/12/30 02:46:00 guenther Exp $	*/
/*	$NetBSD: mem.c,v 1.11 2003/10/16 12:02:58 jdolecek Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1990, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * the Systems Programming Group of the University of Utah Computer
 * Science Department.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */

/*
 * Copyright (c) 1988 University of Utah.
 *
 * This code is derived from software contributed to Berkeley by
 * the Systems Programming Group of the University of Utah Computer
 * Science Department.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by the University of
 *	California, Berkeley and its contributors.
 * 4. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

//! The memory special file (`/dev/mem`, `/dev/kmem`, `/dev/null`, `/dev/zero`):
//! `arch/arm64/arm64/mem.c`.
//!
//! Upstream: sys/arch/arm64/arm64/mem.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `option APERTURE` (minor 4) is not configured.
//! - `/dev/mem` (minor 0): the C maps each physical page at `vmmap` (one reserved page) with
//!   `pmap_enter` under `physlock`. Here `vmmap` is the device mapping window
//!   (`arm64/pmap.rs`), and the kernel has a direct map, which the C's arm64 does not; the
//!   page is reached through it, a page at a time under `physlock` as in C, and copied
//!   through a small buffer with `kcopy`, so an address with nothing behind it fails with
//!   `EFAULT`.
//! - `/dev/kmem` (minor 1) checks the range with `uvm_kernacc` (`uvm_glue.c`), which is not
//!   ported: the read or write is reported and fails.
//! - `/dev/zero` reads come from a zeroed buffer on the stack, `MEM_CHUNK` bytes per step,
//!   instead of the shared `malloc`ed `zeropage` (a slice of it per reader would alias).

use core::sync::atomic::Ordering;

use crate::arch::arm64::arm64::pmap::pmap_direct_map;
use crate::kern::kern_prot::suser;
use crate::kern::kern_rwlock::{rw_enter, rw_exit};
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_sysctl::{ALLOWKMEM, SECURELEVEL};
use crate::kern::subr_prf::panic;
use crate::machine::copy::kcopy;
use crate::machine::cpu::curproc;
use crate::sys::errno::Errno;
use crate::sys::filio::FIOASYNC;
use crate::sys::param::{PAGE_SIZE, PGOFSET};
use crate::sys::proc::Proc;
use crate::sys::rwlock::{RW_INTR, RW_WRITE, Rwlock};
use crate::sys::systm::PHYSMEM;
use crate::sys::types::{Dev, Off, Paddr, minor};
use crate::sys::uio::{Uio, UioRw};
use crate::unported;

/// The bytes `/dev/mem` and `/dev/zero` move per step.
const MEM_CHUNK: usize = 512;

/// `physlock`: lock against other uses of shared vmmap.
static PHYSLOCK: Rwlock = Rwlock::new("mmrw");

/// `mmopen`.
pub fn mmopen(dev: Dev, _flag: i32, _mode: i32, _p: &Proc) -> Result<(), Errno> {
    match minor(dev) {
        0 | 1 => {
            if SECURELEVEL.load(Ordering::Relaxed) <= 0 || ALLOWKMEM.load(Ordering::Relaxed) != 0 {
                Ok(())
            } else {
                Err(Errno::EPERM)
            }
        }
        2 | 12 => Ok(()),
        // APERTURE (minor 4): not configured.
        _ => Err(Errno::ENXIO),
    }
}

/// `mmclose`.
pub fn mmclose(_dev: Dev, _flag: i32, _mode: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    // APERTURE: not configured.
    Ok(())
}

/// `/dev/mem`: moves up to the end of the page at `uio_offset` (and at most `MEM_CHUNK`
/// bytes) between physical memory and the uio.
fn mm_physrw(uio: &mut Uio<'_>) -> Result<(), Errno> {
    let mut buf = [0u8; MEM_CHUNK];
    let o = uio.uio_offset as usize & PGOFSET;
    let c = uio.uio_resid.min(PAGE_SIZE - o).min(MEM_CHUNK);
    let v = pmap_direct_map(Paddr::new(uio.uio_offset as usize)).as_usize();

    if uio.uio_rw == UioRw::UIO_READ {
        // SAFETY: `kcopy` catches a fault on the direct-map side (`EFAULT`); `buf` is ours.
        unsafe { kcopy(v as *const u8, buf.as_mut_ptr(), c) }?;
        uiomove(&mut buf[..c], uio)
    } else {
        uiomove(&mut buf[..c], uio)?;
        // SAFETY: as above, in the other direction; `/dev/mem` writes are the superuser's
        // (`mmopen`'s securelevel check), as in C.
        unsafe { kcopy(buf.as_ptr(), v as *mut u8, c) }
    }
}

/// `mmrw` (also `mmread` and `mmwrite`).
pub fn mmrw(dev: Dev, uio: &mut Uio<'_>, _flags: i32) -> Result<(), Errno> {
    let mut error = Ok(());

    if minor(dev) == 0 {
        // lock against other uses of shared vmmap
        rw_enter(&PHYSLOCK, RW_WRITE | RW_INTR)?;
    }
    while uio.uio_resid > 0 && error.is_ok() {
        let Some(iov) = uio.uio_iov.first() else {
            panic(format_args!("mmrw"));
        };
        let iov_len = iov.iov_len;
        if iov_len == 0 {
            let iov = core::mem::take(&mut uio.uio_iov);
            uio.uio_iov = &mut iov[1..];
            continue;
        }
        match minor(dev) {
            0 => error = mm_physrw(uio),

            1 => {
                // v = uio->uio_offset; c = ulmin(iov->iov_len, MAXPHYS); uvm_kernacc, then
                // uiomove(v, c, uio).
                return Err(unported!("mmrw: /dev/kmem (uvm_kernacc, uvm_glue.c)"));
            }

            2 => {
                if uio.uio_rw == UioRw::UIO_WRITE {
                    uio.uio_resid = 0;
                }
                return Ok(());
            }

            12 => {
                if uio.uio_rw == UioRw::UIO_WRITE {
                    uio.uio_resid = 0;
                    return Ok(());
                }
                let mut zeros = [0u8; MEM_CHUNK];
                let c = iov_len.min(MEM_CHUNK);
                error = uiomove(&mut zeros[..c], uio);
            }

            _ => return Err(Errno::ENXIO),
        }
    }
    if minor(dev) == 0 {
        rw_exit(&PHYSLOCK);
    }
    error
}

/// `mmmmap`: `/dev/mem` is the only one that makes sense through this interface. For
/// `/dev/kmem` any physaddr we return here could be transient and hence incorrect or invalid
/// at a later time. `/dev/null` just doesn't make any sense and `/dev/zero` is a hack that is
/// handled via the default pager in mmap().
pub fn mmmmap(dev: Dev, off: Off, _prot: i32) -> Option<Paddr> {
    let p = curproc(); // XXX

    if minor(dev) != 0 {
        return None;
    }

    // minor device 0 is physical memory

    if off as usize >= PHYSMEM.load(Ordering::Relaxed) * PAGE_SIZE
        && p.is_none_or(|p| suser(p).is_err())
    {
        return None;
    }
    Some(Paddr::new(off as usize))
}

/// `mmioctl`.
pub fn mmioctl(_dev: Dev, cmd: u64, _data: &mut [u8], _flags: i32, _p: &Proc) -> Result<(), Errno> {
    if cmd == FIOASYNC {
        // handled by fd layer
        return Ok(());
    }

    Err(Errno::ENOTTY)
}
