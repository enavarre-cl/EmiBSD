/* <LICENSES> */
/*-
 * Copyright (c) 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * The Mach Operating System project at Carnegie-Mellon University.
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
 *
 *
 * Copyright (c) 1990, 1991 Carnegie Mellon University
 * All Rights Reserved.
 *
 * Author: David Golub
 *
 * Permission to use, copy, modify and distribute this software and its
 * documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND FOR
 * ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie the
 * rights to redistribute these changes.
 */

/*
 *	Stand-alone file reading package.
 */

/*-
 * Copyright (c) 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *
 *	@(#)ufs.h	8.1 (Berkeley) 6/11/93
 */
/* </LICENSES> */

/* <CODE> */
//! Stand-alone file reading package for FFS2 (`struct ufs2_dinode`).
//!
//! Upstream: sys/lib/libsa/ufs2.c @ 3ce1f3f79392, sys/lib/libsa/ufs2.h @ 3ce1f3f79392
//!
//! `ufs2.c` is `ufs.c` with the FFS2 inode, 64-bit block addresses (`daddr_t`), the
//! super-block at `SBLOCK_UFS2` and `FS_UFS2_MAGIC`; here it is `ufs.rs`'s reader
//! ([`Ufs`]) instantiated for [`Ufs2Dinode`], with the `ufs2_*` entry points.
//!
//! ## Deviations
//! - As `ufs.rs`'s.

use crate::hdr::dinode::{NDADDR, NIADDR, Ufs2Dinode};
use crate::hdr::fs::{FS_UFS2_MAGIC, Fs, SBLOCK_UFS2};
use crate::hdr::param::DEV_BSIZE;
use crate::hdr::stat::Stat;
use crate::hdr::types::{Daddr, Mode, Off};
use crate::saerrno::Errno;
use crate::stand::OpenFile;
use crate::ufs::{Dinode, Ufs};

impl Dinode for Ufs2Dinode {
    const SBLOCK: Daddr = SBLOCK_UFS2 / DEV_BSIZE as Daddr;
    const MAGIC: i32 = FS_UFS2_MAGIC;
    const DADDR_SIZE: usize = 8;

    fn mode(&self) -> u16 {
        self.di_mode
    }

    fn set_mode(&mut self, mode: u16) {
        self.di_mode = mode;
    }

    fn size(&self) -> u64 {
        self.di_size
    }

    fn uid(&self) -> u32 {
        self.di_uid
    }

    fn gid(&self) -> u32 {
        self.di_gid
    }

    fn db(&self, i: usize) -> Daddr {
        self.di_db[i]
    }

    fn ib(&self, i: usize) -> Daddr {
        self.di_ib[i]
    }

    fn shortlink(&self) -> &[u8] {
        // SAFETY: `di_db` and `di_ib` are adjacent `i64` arrays in a `#[repr(C)]` struct
        // (offsets 112 and 208), so their 120 bytes are initialised and contiguous.
        unsafe {
            core::slice::from_raw_parts(self.di_db.as_ptr().cast::<u8>(), 8 * (NDADDR + NIADDR))
        }
    }

    fn fsbtodb(fs: &Fs, b: Daddr) -> Daddr {
        b << fs.fs_fsbtodb
    }

    fn indirect(buf: &[u8], i: usize) -> Daddr {
        let b = &buf[8 * i..8 * i + 8];
        Daddr::from_ne_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
    }
}

/// `ufs2_open`.
pub fn ufs2_open(path: &[u8], f: &mut OpenFile) -> Result<(), Errno> {
    Ufs::<Ufs2Dinode>::open(path, f)
}

/// `ufs2_close`.
pub fn ufs2_close(f: &mut OpenFile) -> Result<(), Errno> {
    Ufs::<Ufs2Dinode>::close(f)
}

/// `ufs2_read`.
pub fn ufs2_read(f: &mut OpenFile, start: &mut [u8], resid: &mut usize) -> Result<(), Errno> {
    Ufs::<Ufs2Dinode>::read(f, start, resid)
}

/// `ufs2_write`.
pub fn ufs2_write(f: &mut OpenFile, start: &[u8], resid: &mut usize) -> Result<(), Errno> {
    Ufs::<Ufs2Dinode>::write(f, start, resid)
}

/// `ufs2_seek`.
pub fn ufs2_seek(f: &mut OpenFile, offset: Off, whence: i32) -> Result<Off, Errno> {
    Ufs::<Ufs2Dinode>::seek(f, offset, whence)
}

/// `ufs2_stat`.
pub fn ufs2_stat(f: &mut OpenFile, sb: &mut Stat) -> Result<(), Errno> {
    Ufs::<Ufs2Dinode>::stat(f, sb)
}

/// `ufs2_readdir`.
pub fn ufs2_readdir(f: &mut OpenFile, name: Option<&mut [u8]>) -> Result<(), Errno> {
    Ufs::<Ufs2Dinode>::readdir(f, name)
}

/// `ufs2_fchmod`.
pub fn ufs2_fchmod(f: &mut OpenFile, mode: Mode) -> Result<(), Errno> {
    Ufs::<Ufs2Dinode>::fchmod(f, mode)
}
/* </CODE> */
