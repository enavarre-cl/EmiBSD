/*	$OpenBSD: msgbuf.h,v 1.13 2020/10/25 10:55:42 visa Exp $	*/
/*	$NetBSD: msgbuf.h,v 1.8 1995/03/26 20:24:27 jtc Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1981, 1984, 1993
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
 *	@(#)msgbuf.h	8.1 (Berkeley) 6/2/93
 */
/* </LICENSES> */

/* <CODE> */
//! The kernel message buffer that `dmesg(8)` reads: `<sys/msgbuf.h>`.
//!
//! Upstream: sys/sys/msgbuf.h @ 3ce1f3f79392
//!
//! A `struct msgbuf` is laid over a memory area the machine code chooses (it survives a warm
//! reboot); the header is followed by `msg_bufs` bytes of ring buffer. The functions that fill
//! and read it are in `kern/subr_log.rs`.
//!
//! Locking, as in the C: `I` immutable after creation, `L` `log_mtx`, `Lw` `log_mtx` for
//! writing (`LOG_MTX`, `kern/subr_log.rs`, since M11e).
//!
//! ## Deviations
//! - `msg_bufc[1]` (a flexible array in disguise) is a real unsized tail; [`Msgbuf::from_raw`]
//!   lays the structure over a byte area, which is what the C cast does.

use core::cell::Cell;
use core::ptr;

/// `MSG_MAGIC`: buffer magic value.
pub const MSG_MAGIC: i64 = 0x063061;
/// `CONSBUFSIZE`: console message buffer size.
pub const CONSBUFSIZE: usize = 16 * 1024;

/// `struct msgbuf`.
#[repr(C)]
pub struct Msgbuf {
    /// [I] buffer magic value.
    msg_magic: Cell<i64>,
    /// [L] write pointer.
    msg_bufx: Cell<i64>,
    /// [L] read pointer.
    msg_bufr: Cell<i64>,
    /// [I] real `msg_bufc` size (bytes).
    msg_bufs: Cell<i64>,
    /// [L] number of dropped bytes.
    msg_bufd: Cell<i64>,
    /// [Lw] buffer.
    msg_bufc: [Cell<u8>],
}

// SAFETY: the fields are written under `log_mtx` and the `[L]` ones read under it, as in C.
unsafe impl Sync for Msgbuf {}

impl Msgbuf {
    /// `offsetof(struct msgbuf, msg_bufc)`: the header before the ring.
    pub const HEADER_SIZE: usize = 5 * core::mem::size_of::<i64>();
    /// `sizeof(struct msgbuf)`: the header and `msg_bufc[1]`, padded to the `long` alignment;
    /// the smallest area `initmsgbuf` accepts.
    pub const MIN_SIZE: usize = Self::HEADER_SIZE + core::mem::size_of::<i64>();

    /// Lays a `Msgbuf` over `bufsize` bytes at `buf`, as the C cast does. Nothing is
    /// initialised; `initmsgbuf` in `kern/subr_log.rs` validates or resets the header.
    ///
    /// # Safety
    ///
    /// `buf` is 8-byte aligned, valid for reads and writes of `bufsize >= HEADER_SIZE + 1`
    /// bytes for the rest of the kernel's life, and not accessed through any other path.
    pub unsafe fn from_raw(buf: *mut u8, bufsize: usize) -> &'static Msgbuf {
        let ring = bufsize - Self::HEADER_SIZE;
        let fat: *mut [u8] = ptr::slice_from_raw_parts_mut(buf, ring);
        // SAFETY: a `#[repr(C)]` struct with an unsized tail takes the tail's element count as
        // its pointer metadata; the caller guarantees the area is large and aligned enough.
        unsafe { &*(fat as *mut Msgbuf) }
    }

    /// `msg_magic`.
    pub fn magic(&self) -> i64 {
        self.msg_magic.get()
    }

    /// Sets `msg_magic`.
    pub fn set_magic(&self, v: i64) {
        self.msg_magic.set(v);
    }

    /// `msg_bufx`: write pointer.
    pub fn bufx(&self) -> i64 {
        self.msg_bufx.get()
    }

    /// Sets `msg_bufx`.
    pub fn set_bufx(&self, v: i64) {
        self.msg_bufx.set(v);
    }

    /// `msg_bufr`: read pointer.
    pub fn bufr(&self) -> i64 {
        self.msg_bufr.get()
    }

    /// Sets `msg_bufr`.
    pub fn set_bufr(&self, v: i64) {
        self.msg_bufr.set(v);
    }

    /// `msg_bufs`: size of the ring in bytes.
    pub fn bufs(&self) -> i64 {
        self.msg_bufs.get()
    }

    /// Sets `msg_bufs`.
    pub fn set_bufs(&self, v: i64) {
        self.msg_bufs.set(v);
    }

    /// `msg_bufd`: bytes dropped because the ring was full.
    pub fn bufd(&self) -> i64 {
        self.msg_bufd.get()
    }

    /// Sets `msg_bufd`.
    pub fn set_bufd(&self, v: i64) {
        self.msg_bufd.set(v);
    }

    /// `msg_bufc`: the ring.
    pub fn bufc(&self) -> &[Cell<u8>] {
        &self.msg_bufc
    }

    /// Zeroes header and ring (`memset(buf, 0, bufsize)`).
    pub fn clear(&self) {
        self.msg_magic.set(0);
        self.msg_bufx.set(0);
        self.msg_bufr.set(0);
        self.msg_bufs.set(0);
        self.msg_bufd.set(0);
        for b in &self.msg_bufc {
            b.set(0);
        }
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C, align(8))]
    struct Area([u8; 128]);

    #[test]
    fn overlay_matches_the_c_layout() {
        let mut area = Area([0xaa; 128]);
        // SAFETY: the area is aligned, large enough and used only through the overlay.
        let mb = unsafe { Msgbuf::from_raw(area.0.as_mut_ptr(), 128) };
        assert_eq!(Msgbuf::HEADER_SIZE, 40);
        assert_eq!(Msgbuf::MIN_SIZE, 48);
        assert_eq!(mb.bufc().len(), 128 - 40);
        mb.set_magic(MSG_MAGIC);
        mb.set_bufs(88);
        mb.bufc()[0].set(b'x');
        let base = area.0.as_ptr() as usize;
        assert_eq!(mb as *const Msgbuf as *const u8 as usize, base);
        assert_eq!(mb.bufc().as_ptr() as usize, base + 40);
        assert_eq!(area.0[40], b'x');
        assert_eq!(
            i64::from_ne_bytes(area.0[0..8].try_into().unwrap()),
            MSG_MAGIC
        );
        assert_eq!(i64::from_ne_bytes(area.0[24..32].try_into().unwrap()), 88);
    }
}
/* </TESTS> */
