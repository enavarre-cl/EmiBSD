/*	$OpenBSD: subr_log.c,v 1.81 2025/06/03 00:20:31 dlg Exp $	*/
/*	$NetBSD: subr_log.c,v 1.11 1996/03/30 22:24:44 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1993
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
 *	@(#)subr_log.c	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

//! Error log buffer for kernel printf's: `kern/subr_log.c`.
//!
//! Upstream: sys/kern/subr_log.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports the message buffer itself: `initmsgbuf`,
//! `msgbuf_putchar`, `msgbuf_putchar_locked`, `logwakeup`, `initconsbuf` and the globals
//! `log_open`, `msgbufmapped`, `msgbufp`, `consbufp`. The `/dev/klog` device (`logopen` through
//! `logkqfilter`, `logtick`, `dosendsyslog`, `sys_sendsyslog`) needs files, sockets and kqueue
//! (M7).
//!
//! ## Deviations
//! - `log_mtx` arrives with M5; until then the single boot CPU is the lock, and
//!   `msgbuf_putchar` goes straight to `msgbuf_putchar_locked`.
//! - `msgbufp` is a [`StaticCell`] (a `&'static Msgbuf` is a fat pointer, which no atomic holds).
//! - [`init_static_msgbuf`] is ours: until `pmap` (M3) reserves physical pages that survive a
//!   warm reboot, the message buffer is a static area in `.bss`, and both architectures hand it
//!   to `initmsgbuf` from their early init.
//! - `logsoftc` is reduced to its `sc_need_wakeup` flag, the only member `logwakeup` touches.
//! - `initconsbuf` needs `malloc(9)` (M3) and reports the gap instead.

use core::sync::atomic::{AtomicBool, Ordering, fence};

use libkern::StaticCell;

use crate::machine::{Machine, MachineParam};
use crate::sys::msgbuf::{CONSBUFSIZE, MSG_MAGIC, Msgbuf};
use crate::unported;

/// `MSGBUFSIZE` of the selected machine: the static buffer's size.
const MSGBUFSIZE: usize = <Machine as MachineParam>::MSGBUFSIZE;

/// The static message buffer area (see the module's deviations).
#[repr(C, align(8))]
struct MsgbufArea([u8; MSGBUFSIZE]);

/// `log_open`: is `/dev/klog` open? Also used in `log()`.
pub static LOG_OPEN: AtomicBool = AtomicBool::new(false);
/// `msgbufmapped`: is the message buffer mapped?
static MSGBUFMAPPED: AtomicBool = AtomicBool::new(false);
/// `msgbufp`: the mapped buffer, itself.
static MSGBUFP: StaticCell<Option<&'static Msgbuf>> = StaticCell::new(None);
/// `consbufp`: console message buffer.
static CONSBUFP: StaticCell<Option<&'static Msgbuf>> = StaticCell::new(None);
/// `logsoftc.sc_need_wakeup`: if set, wake up waiters.
static LOGSOFTC_NEED_WAKEUP: AtomicBool = AtomicBool::new(false);
/// The buffer `init_static_msgbuf` overlays.
static MSGBUF_AREA: StaticCell<MsgbufArea> = StaticCell::new(MsgbufArea([0; MSGBUFSIZE]));

/// `msgbufmapped`: whether `initmsgbuf` has run.
pub fn msgbufmapped() -> bool {
    MSGBUFMAPPED.load(Ordering::Acquire)
}

/// `msgbufp`: the message buffer, once mapped.
pub fn msgbufp() -> Option<&'static Msgbuf> {
    // SAFETY: written once by `initmsgbuf`, on the boot CPU before anything prints; only read
    // afterwards.
    unsafe { MSGBUFP.read() }
}

/// `consbufp`: the console buffer, once `initconsbuf` has run.
pub fn consbufp() -> Option<&'static Msgbuf> {
    // SAFETY: as for `msgbufp`.
    unsafe { CONSBUFP.read() }
}

/// `initmsgbuf`: lays the message buffer over `bufsize` bytes at `buf`. A header left by a
/// previous boot (right magic, same size, consistent pointers) is kept, so `dmesg(8)` can show
/// what happened before a reboot; otherwise the area is cleared and initialised. New output
/// always starts on a fresh line.
///
/// # Safety
///
/// As for [`Msgbuf::from_raw`]: `buf` is 8-byte aligned, valid for `bufsize` bytes for the rest
/// of the kernel's life, and used through nothing but the message buffer.
pub unsafe fn initmsgbuf(buf: *mut u8, bufsize: usize) {
    // Sanity-check the given size.
    if bufsize < Msgbuf::MIN_SIZE {
        return;
    }

    // SAFETY: forwarded from the caller.
    let mbp = unsafe { Msgbuf::from_raw(buf, bufsize) };
    // SAFETY: single writer, on the boot CPU, before any reader (see `msgbufp`).
    unsafe { MSGBUFP.write(Some(mbp)) };

    let new_bufs = (bufsize - Msgbuf::HEADER_SIZE) as i64;
    if mbp.magic() != MSG_MAGIC
        || mbp.bufs() != new_bufs
        || mbp.bufr() < 0
        || mbp.bufr() >= mbp.bufs()
        || mbp.bufx() < 0
        || mbp.bufx() >= mbp.bufs()
    {
        // If the buffer magic number is wrong, has changed size (which shouldn't happen
        // often), or is internally inconsistent, initialize it.
        mbp.clear();
        mbp.set_magic(MSG_MAGIC);
        mbp.set_bufs(new_bufs);
    }

    // Always start new buffer data on a new line. Avoid using log_mtx because mutexes do not
    // work during early boot on some architectures.
    if mbp.bufx() > 0 && mbp.bufc()[(mbp.bufx() - 1) as usize].get() != b'\n' {
        msgbuf_putchar_locked(mbp, b'\n');
    }

    // mark it as ready for use.
    MSGBUFMAPPED.store(true, Ordering::Release);
}

/// Hands the static buffer area to [`initmsgbuf`] (see the module's deviations).
pub fn init_static_msgbuf() {
    // SAFETY: the area is 8-byte aligned, `MSGBUFSIZE` bytes long, lives forever and is only
    // ever reached through the overlay `initmsgbuf` installs.
    unsafe { initmsgbuf(MSGBUF_AREA.as_ptr().cast::<u8>(), MSGBUFSIZE) }
}

/// `initconsbuf`: sets up a buffer to collect `/dev/console` output.
pub fn initconsbuf() {
    // consbufp = malloc(CONSBUFSIZE, M_TTYS, M_WAITOK | M_ZERO): malloc(9) arrives with M3.
    let _ = CONSBUFSIZE;
    let _ = unported!("malloc (initconsbuf)");
    // SAFETY: single writer, on the boot CPU (see `consbufp`).
    unsafe { CONSBUFP.write(None) };
}

/// `msgbuf_putchar`: appends `c` to `mbp` under `log_mtx`; nothing happens if the buffer was
/// never initialised.
pub fn msgbuf_putchar(mbp: &Msgbuf, c: u8) {
    if mbp.magic() != MSG_MAGIC {
        // Nothing we can do
        return;
    }
    // mtx_enter(&log_mtx) ... mtx_leave(&log_mtx): M5.
    msgbuf_putchar_locked(mbp, c);
}

/// `msgbuf_putchar_locked`: appends `c` to the ring; when it is full the oldest byte is
/// dropped and counted in `msg_bufd`.
pub fn msgbuf_putchar_locked(mbp: &Msgbuf, c: u8) {
    let bufc = mbp.bufc();
    let mut x = mbp.bufx();
    bufc[x as usize].set(c);
    x += 1;
    if x < 0 || x >= mbp.bufs() {
        x = 0;
    }
    mbp.set_bufx(x);
    // If the buffer is full, keep the most recent data.
    if mbp.bufr() == x {
        let mut r = mbp.bufr() + 1;
        if r >= mbp.bufs() {
            r = 0;
        }
        mbp.set_bufr(r);
        mbp.set_bufd(mbp.bufd() + 1);
    }
}

/// `logwakeup`: asks for the `/dev/klog` readers to be woken. The actual wakeup has to be
/// deferred because `logwakeup()` can be called in very varied contexts; keeping the print
/// routines usable in as many situations as possible means no locking here.
pub fn logwakeup() {
    // Ensure that preceding stores become visible to other CPUs before the flag
    // (membar_producer).
    fence(Ordering::Release);
    LOGSOFTC_NEED_WAKEUP.store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C, align(8))]
    struct Area([u8; 64]);

    #[test]
    fn ring_wraps_and_drops_the_oldest() {
        let mut area = Area([0; 64]);
        // SAFETY: the area outlives the test and is used only through the overlay.
        let mbp = unsafe { Msgbuf::from_raw(area.0.as_mut_ptr(), 64) };
        mbp.clear();
        mbp.set_magic(MSG_MAGIC);
        mbp.set_bufs(24);
        for b in b"0123456789abcdefghijklm" {
            msgbuf_putchar(mbp, *b);
        }
        assert_eq!(mbp.bufx(), 23);
        assert_eq!(mbp.bufr(), 0);
        assert_eq!(mbp.bufd(), 0);
        msgbuf_putchar(mbp, b'n'); // fills the ring: bufx wraps to 0 == bufr
        assert_eq!(mbp.bufx(), 0);
        assert_eq!(mbp.bufr(), 1);
        assert_eq!(mbp.bufd(), 1);
        msgbuf_putchar(mbp, b'o'); // still full: the read pointer keeps running ahead
        assert_eq!(mbp.bufc()[0].get(), b'o');
        assert_eq!(mbp.bufr(), 2);
        assert_eq!(mbp.bufd(), 2);
        // The wrong magic makes the buffer inert.
        mbp.set_magic(0);
        msgbuf_putchar(mbp, b'p');
        assert_eq!(mbp.bufx(), 1);
    }

    #[test]
    fn initmsgbuf_keeps_a_sane_header_and_resets_a_bad_one() {
        let mut area = Area([0xff; 64]);
        let p = area.0.as_mut_ptr();
        // SAFETY: the area outlives the test and is used only through the overlay.
        unsafe { initmsgbuf(p, 64) };
        let mbp = msgbufp().unwrap();
        assert_eq!(mbp.magic(), MSG_MAGIC);
        assert_eq!(mbp.bufs(), 24);
        assert_eq!(mbp.bufx(), 0);
        assert!(msgbufmapped());
        msgbuf_putchar(mbp, b'a');
        // SAFETY: as above; a second init over the same bytes sees a valid header.
        unsafe { initmsgbuf(p, 64) };
        let mbp = msgbufp().unwrap();
        assert_eq!(mbp.bufx(), 2, "a newline was appended after the kept 'a'");
        assert_eq!(mbp.bufc()[1].get(), b'\n');
        // SAFETY: as above; too small an area is refused.
        unsafe { initmsgbuf(p, 8) };
        logwakeup();
        init_static_msgbuf();
        assert_eq!(
            msgbufp().unwrap().bufs(),
            (MSGBUFSIZE - Msgbuf::HEADER_SIZE) as i64
        );
    }
}
