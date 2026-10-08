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

/* <CODE> */
//! Error log buffer for kernel printf's: `kern/subr_log.c`.
//!
//! Upstream: sys/kern/subr_log.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports the message buffer itself: `initmsgbuf`,
//! `msgbuf_putchar`, `msgbuf_putchar_locked`, `logwakeup`, `initconsbuf` and the globals
//! `log_open`, `msgbufmapped`, `msgbufp`, `consbufp`. M8: `sendsyslog(2)`: the log stash
//! (`logstash_full`, `logstash_increment`, `logstash_insert`, `logstash_remove`,
//! `logstash_sendsyslog`), `sys_sendsyslog` and `dosendsyslog`, with `syslogf` and its
//! rwlock. The `/dev/klog` device (`logopen` through `logkqfilter`, `logtick`, `logioctl`
//! with `LIOCSFD`) needs the device switch, sockets and kqueue. M11e: `log_mtx`, taken by
//! `msgbuf_putchar` as in C.
//!
//! ## Deviations
//! - `msgbufp` is a [`StaticCell`] (a `&'static Msgbuf` is a fat pointer, which no atomic holds).
//! - [`init_static_msgbuf`] is ours: until `pmap` (M3) reserves physical pages that survive a
//!   warm reboot, the message buffer is a static area in `.bss`, and both architectures hand it
//!   to `initmsgbuf` from their early init.
//! - `initmsgbuf`'s check and repair of the header is [`initmsgbuf_header`], which works on a
//!   given overlay without installing it as `msgbufp`, so the host tests exercise it on a
//!   private buffer: tests run in parallel and print through the global one.
//! - `logsoftc` is reduced to its `sc_need_wakeup` flag, the only member `logwakeup` touches.
//! - `initconsbuf` needs `malloc(9)` (M3) and reports the gap instead.
//! - Nothing sets `syslogf` (that is `logioctl(LIOCSFD)`'s job, with the log device), so
//!   `dosendsyslog` writes a `LOG_CONS` message to the console through `cnputc` (`constty`
//!   and `cn_devvp` are NULL without the tty layer) and answers `ENOTCONN`, and the other
//!   messages are stashed; once a socket is set, the message goes to it through `sosend`. The
//!   stash's cursors are indices into the ring, not pointers.

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering, fence};

use libkern::StaticCell;

use crate::dev::cons::cnputc;
use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::{
    rw_assert_anylock, rw_assert_wrlock, rw_enter_read, rw_enter_write, rw_exit,
};
use crate::kern::subr_prf::snprintf;
use crate::kern::sys_socket::fp_socket;
use crate::kern::uipc_socket::sosend;
use crate::machine::copy::copyin;
use crate::machine::intr::IPL_HIGH;
use crate::machine::{Machine, MachineParam};
use crate::sys::errno::Errno;
use crate::sys::fcntl::FNONBLOCK;
use crate::sys::file::{File, fref, frele};
use crate::sys::malloc::{M_LOG, M_WAITOK};
use crate::sys::msgbuf::{CONSBUFSIZE, MSG_MAGIC, Msgbuf};
use crate::sys::mutex::Mutex;
use crate::sys::proc::Proc;
use crate::sys::rwlock::Rwlock;
use crate::sys::socket::MSG_DONTWAIT;
use crate::sys::syscallargs::SysSendsyslogArgs;
use crate::sys::syslog::{LOG_CONS, LOG_KERN, LOG_MAXLINE, LOG_WARNING};
use crate::sys::systm::{SysArgs, kernel_lock, kernel_unlock, sysargs};
use crate::sys::types::{Pid, Register};
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
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
/// `log_mtx`: serializes access to the log message buffers. This should be kept as a leaf
/// lock in order not to constrain where printf(9) can be used (`MTX_NOWITNESS` in C).
pub static LOG_MTX: Mutex = Mutex::new(IPL_HIGH);
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

    initmsgbuf_header(mbp, bufsize);

    // mark it as ready for use.
    MSGBUFMAPPED.store(true, Ordering::Release);
}

/// The header half of [`initmsgbuf`]: keeps a sane header left by a previous boot, clears and
/// initialises any other, and starts new output on a fresh line (see the module's
/// deviations).
fn initmsgbuf_header(mbp: &Msgbuf, bufsize: usize) {
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
    mtx_enter(&LOG_MTX);
    msgbuf_putchar_locked(mbp, c);
    mtx_leave(&LOG_MTX);
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

/// `LOGSTASH_SIZE`: how many messages the log stash keeps while `syslogd(8)` is away.
const LOGSTASH_SIZE: usize = 100;

/// `struct logstash_message`: one stashed message (`malloc(M_LOG)` bytes, or none).
struct LogstashMessage {
    /// `lgs_buffer`.
    lgs_buffer: Cell<Option<NonNull<u8>>>,
    /// `lgs_size`.
    lgs_size: Cell<usize>,
}

/// The log stash: `logstash_messages[]`, the `logstash_in`/`logstash_out` cursors (indices
/// into the ring) and the drop bookkeeping (`logstash_dropped`, `logstash_error`,
/// `logstash_pid`). Protected by: `logstash_rwlock`.
struct Logstash {
    /// `logstash_messages`.
    messages: [LogstashMessage; LOGSTASH_SIZE],
    /// `logstash_in`.
    r#in: Cell<usize>,
    /// `logstash_out`.
    out: Cell<usize>,
    /// `logstash_dropped`.
    dropped: Cell<i32>,
    /// `logstash_error`.
    error: Cell<i32>,
    /// `logstash_pid`.
    pid: Cell<Pid>,
}

// SAFETY: every member is touched only under `logstash_rwlock` (`LOGSTASH_RWLOCK`) held for
// writing, as in C.
unsafe impl Sync for Logstash {}

/// The log stash, empty.
static LOGSTASH: Logstash = Logstash {
    messages: [const {
        LogstashMessage {
            lgs_buffer: Cell::new(None),
            lgs_size: Cell::new(0),
        }
    }; LOGSTASH_SIZE],
    r#in: Cell::new(0),
    out: Cell::new(0),
    dropped: Cell::new(0),
    error: Cell::new(0),
    pid: Cell::new(0),
};

/// `logstash_rwlock`.
static LOGSTASH_RWLOCK: Rwlock = Rwlock::new("logstash");

/// `syslogf`: the socket `syslogd(8)` hands the log device (`LIOCSFD`); `None` while there
/// is none, which is always until the log device and sockets are ported.
static SYSLOGF: AtomicPtr<File> = AtomicPtr::new(ptr::null_mut());
/// `syslogf_rwlock`.
static SYSLOGF_RWLOCK: Rwlock = Rwlock::new("syslogf");

/// `logstash_full`.
fn logstash_full() -> bool {
    rw_assert_anylock(&LOGSTASH_RWLOCK);

    LOGSTASH.messages[LOGSTASH.out.get()]
        .lgs_buffer
        .get()
        .is_some()
        && LOGSTASH.r#in.get() == LOGSTASH.out.get()
}

/// `logstash_increment`: advances a cursor around the ring.
fn logstash_increment(cursor: &Cell<usize>) {
    rw_assert_wrlock(&LOGSTASH_RWLOCK);

    kassert!(cursor.get() < LOGSTASH_SIZE);
    if cursor.get() == LOGSTASH_SIZE - 1 {
        cursor.set(0);
    } else {
        cursor.set(cursor.get() + 1);
    }
}

/// `logstash_insert`: keeps a copy of the user message `buf` for later, or counts it as
/// dropped when the stash is full.
pub fn logstash_insert(buf: usize, nbyte: usize, logerror: Errno, pid: Pid) -> Result<(), Errno> {
    rw_enter_write(&LOGSTASH_RWLOCK);

    if logstash_full() {
        if LOGSTASH.dropped.get() == 0 {
            LOGSTASH.error.set(logerror as i32);
            LOGSTASH.pid.set(pid);
        }
        LOGSTASH.dropped.set(LOGSTASH.dropped.get() + 1);

        rw_exit(&LOGSTASH_RWLOCK);
        return Ok(());
    }

    let slot = &LOGSTASH.messages[LOGSTASH.r#in.get()];
    let Some(mem) = malloc(nbyte.max(1), M_LOG, M_WAITOK) else {
        rw_exit(&LOGSTASH_RWLOCK);
        return Err(Errno::ENOMEM);
    };
    // SAFETY: a fresh `nbyte`-byte allocation, owned by the stash from here.
    let kbuf = unsafe { core::slice::from_raw_parts_mut(mem.as_ptr(), nbyte) };
    if let Err(error) = copyin(buf, kbuf) {
        free(mem, M_LOG, nbyte.max(1));
        slot.lgs_buffer.set(None);

        rw_exit(&LOGSTASH_RWLOCK);
        return Err(error);
    }
    slot.lgs_buffer.set(Some(mem));
    slot.lgs_size.set(nbyte);
    logstash_increment(&LOGSTASH.r#in);

    rw_exit(&LOGSTASH_RWLOCK);
    Ok(())
}

/// `logstash_remove`: frees the oldest stashed message and, if messages were dropped,
/// stashes a note saying how many in their place in the sequence.
pub fn logstash_remove() {
    rw_assert_wrlock(&LOGSTASH_RWLOCK);

    let out = &LOGSTASH.messages[LOGSTASH.out.get()];
    kassert!(out.lgs_buffer.get().is_some());
    if let Some(buf) = out.lgs_buffer.take() {
        free(buf, M_LOG, out.lgs_size.get().max(1));
    }
    logstash_increment(&LOGSTASH.out);

    // Insert dropped message in sequence where messages were dropped.
    let dropped = LOGSTASH.dropped.get();
    if dropped != 0 {
        let mut buf = [0u8; 80];
        let l = snprintf(
            &mut buf,
            format_args!(
                "<{}>sendsyslog: dropped {} message{}, error {}, pid {}",
                LOG_KERN | LOG_WARNING,
                dropped,
                if dropped == 1 { "" } else { "s" },
                LOGSTASH.error.get(),
                LOGSTASH.pid.get()
            ),
        );
        LOGSTASH.dropped.set(0);
        LOGSTASH.error.set(0);
        LOGSTASH.pid.set(0);

        // Cannot fail, we have just freed a slot.
        kassert!(!logstash_full());
        let nbyte = l.min(buf.len() - 1);
        let slot = &LOGSTASH.messages[LOGSTASH.r#in.get()];
        if let Some(mem) = malloc(nbyte.max(1), M_LOG, M_WAITOK) {
            // SAFETY: a fresh `nbyte`-byte allocation, owned by the stash from here.
            unsafe { ptr::copy_nonoverlapping(buf.as_ptr(), mem.as_ptr(), nbyte) };
            slot.lgs_buffer.set(Some(mem));
            slot.lgs_size.set(nbyte);
            logstash_increment(&LOGSTASH.r#in);
        }
    }
}

/// `logstash_sendsyslog`: sends the stashed messages, oldest first, until one fails.
pub fn logstash_sendsyslog(p: &Proc) -> Result<(), Errno> {
    rw_enter_write(&LOGSTASH_RWLOCK);

    loop {
        let out = &LOGSTASH.messages[LOGSTASH.out.get()];
        let Some(buf) = out.lgs_buffer.get() else {
            break;
        };
        let error = dosendsyslog(p, SyslogBuf::Sys(buf.as_ptr()), out.lgs_size.get(), 0);
        if let Err(error) = error {
            rw_exit(&LOGSTASH_RWLOCK);
            return Err(error);
        }
        logstash_remove();
    }

    rw_exit(&LOGSTASH_RWLOCK);
    Ok(())
}

/// Send syslog(3) message from userland to socketpair(2) created by syslogd(8). Store
/// message in kernel log stash for later if syslogd(8) is not available or sending fails.
/// Send to console if `LOG_CONS` is set and syslogd(8) socket does not exist.
pub fn sys_sendsyslog(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSendsyslogArgs = sysargs(v);

    let nbyte = uap.nbyte.get().min(LOG_MAXLINE);

    let _ = logstash_sendsyslog(p);
    let buf = uap.buf.get() as usize;
    let error = dosendsyslog(p, SyslogBuf::User(buf), nbyte, uap.flags.get());
    if let Err(e) = error
        && e != Errno::EFAULT
    {
        let _ = logstash_insert(buf, nbyte, e, p.process().ps_pid.get());
    }
    error
}

/// The message `dosendsyslog` sends: a user address (`UIO_USERSPACE`) or a kernel buffer
/// (`UIO_SYSSPACE`, a stashed message).
#[derive(Clone, Copy)]
pub enum SyslogBuf {
    /// A user address.
    User(usize),
    /// A kernel buffer of at least `nbyte` bytes.
    Sys(*const u8),
}

impl SyslogBuf {
    /// Copies `out.len()` bytes from `off` on into `out`.
    fn read(self, off: usize, out: &mut [u8]) -> Result<(), Errno> {
        match self {
            SyslogBuf::User(addr) => copyin(addr + off, out),
            SyslogBuf::Sys(ptr) => {
                // SAFETY: a stashed message holds `lgs_size` bytes, and the callers read
                // within `nbyte` of it.
                unsafe { ptr::copy_nonoverlapping(ptr.add(off), out.as_mut_ptr(), out.len()) };
                Ok(())
            }
        }
    }
}

/// `dosendsyslog`: sends `nbyte` bytes of `buf` to `syslogd(8)`'s socket, or, when there is
/// none and `LOG_CONS` is set, to the console without the `<pri>` prefix. `ENOTCONN` when
/// nothing took the message.
pub fn dosendsyslog(p: &Proc, buf: SyslogBuf, nbyte: usize, flags: i32) -> Result<(), Errno> {
    let mut nbyte = nbyte;
    let mut start = 0usize;

    // Global variable syslogf may change during sleep, use local copy.
    rw_enter_read(&SYSLOGF_RWLOCK);
    // SAFETY: a non-null `syslogf` is a file the log device holds a reference on; it is
    // read under `syslogf_rwlock`.
    let fp: Option<&'static File> = unsafe { SYSLOGF.load(Ordering::Acquire).as_ref() };
    if let Some(fp) = fp {
        fref(fp);
    }
    rw_exit(&SYSLOGF_RWLOCK);

    if fp.is_none() {
        if flags & LOG_CONS == 0 {
            return Err(Errno::ENOTCONN);
        }
        // Strip off syslog priority when logging to console. LOG_PRIMASK | LOG_FACMASK is
        // 0x03ff, so at most 4 decimal digits may appear in priority as <1023>.
        let mut pri = [0u8; 6];
        let len = nbyte.min(pri.len());
        buf.read(0, &mut pri[..len])?;
        if 0 < len && pri[0] == b'<' {
            let mut i = 1;
            while i < len {
                if !pri[i].is_ascii_digit() {
                    break;
                }
                i += 1;
            }
            if i < len && pri[i] == b'>' {
                i += 1;
                // There must be at least one digit <0>.
                if i >= 3 {
                    start = i;
                    nbyte -= i;
                }
            }
        }
    }

    // KTRACE: not configured.
    let mut len = nbyte;
    let error = match fp {
        Some(fp) => {
            let (base, segflg) = match buf {
                SyslogBuf::User(addr) => (addr + start, UioSeg::UIO_USERSPACE),
                SyslogBuf::Sys(ptr) => (ptr as usize + start, UioSeg::UIO_SYSSPACE),
            };
            let mut aiov = [Iovec {
                iov_base: ptr::without_provenance_mut(base),
                iov_len: nbyte,
            }];
            let mut auio = Uio {
                uio_iov: &mut aiov,
                uio_offset: 0,
                uio_resid: nbyte,
                uio_segflg: segflg,
                uio_rw: UioRw::UIO_WRITE,
                uio_procp: Some(p),
            };
            let flags = if fp.flag() & FNONBLOCK != 0 {
                MSG_DONTWAIT
            } else {
                0
            };
            let error = sosend(fp_socket(fp), None, Some(&mut auio), None, None, flags);
            if error.is_ok() {
                len -= auio.uio_resid;
            }
            error
        }
        None => {
            kernel_lock(); // KERNEL_LOCK()
            // constty and cn_devvp (the tty layer) are NULL: the console redirection breaks
            // down and the bytes go to cnputc.
            // XXX console redirection breaks down...
            let mut resid = len;
            let mut error = Ok(());
            let mut kbuf = [0u8; 256];
            let mut off = 0;
            'out: while off < len {
                let chunk = (len - off).min(kbuf.len());
                if let Err(e) = buf.read(start + off, &mut kbuf[..chunk]) {
                    error = Err(e);
                    break;
                }
                for &c in &kbuf[..chunk] {
                    if c == 0 {
                        break 'out;
                    }
                    cnputc(i32::from(c));
                    resid -= 1;
                }
                off += chunk;
            }
            if error.is_ok() {
                len -= resid;
            }
            cnputc(i32::from(b'\n'));
            kernel_unlock(); // KERNEL_UNLOCK()
            error
        }
    };
    let _ = len;

    match fp {
        Some(fp) => {
            let _ = frele(fp, p);
            error
        }
        None => match error {
            Err(Errno::EFAULT) => Err(Errno::EFAULT),
            _ => Err(Errno::ENOTCONN),
        },
    }
}
/* </CODE> */

/* <TESTS> */
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
        // A private buffer: the global one is shared with every other test, which may print
        // while this one runs.
        let mut area = Area([0xff; 64]);
        let p = area.0.as_mut_ptr();
        // SAFETY: the area outlives the test and is used only through the overlay.
        let mbp = unsafe { Msgbuf::from_raw(p, 64) };
        initmsgbuf_header(mbp, 64);
        assert_eq!(mbp.magic(), MSG_MAGIC);
        assert_eq!(mbp.bufs(), 24);
        assert_eq!(mbp.bufx(), 0);
        msgbuf_putchar(mbp, b'a');
        // A second init over the same bytes sees a valid header.
        initmsgbuf_header(mbp, 64);
        assert_eq!(mbp.bufx(), 2, "a newline was appended after the kept 'a'");
        assert_eq!(mbp.bufc()[1].get(), b'\n');
        // A header that disagrees with the size is reset.
        mbp.set_bufs(23);
        initmsgbuf_header(mbp, 64);
        assert_eq!((mbp.bufs(), mbp.bufx()), (24, 0));
    }

    #[test]
    fn initmsgbuf_installs_the_static_buffer() {
        // The global buffer: only what concurrent printing cannot change is checked (its
        // size and that it is mapped), and the small area below is refused before anything
        // is installed.
        let mut area = Area([0; 64]);
        // SAFETY: too small: initmsgbuf returns before touching or installing the area.
        unsafe { initmsgbuf(area.0.as_mut_ptr(), 8) };
        logwakeup();
        init_static_msgbuf();
        assert!(msgbufmapped());
        assert_eq!(
            msgbufp().unwrap().bufs(),
            (MSGBUFSIZE - Msgbuf::HEADER_SIZE) as i64
        );
    }
}
/* </TESTS> */
