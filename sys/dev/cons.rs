/*	$OpenBSD: cons.h,v 1.20 2024/05/13 01:15:50 jsg Exp $	*/
/*	$NetBSD: cons.h,v 1.14 1996/03/14 19:08:35 christos Exp $	*/
/*	$OpenBSD: cons.c,v 1.31 2025/09/20 13:53:36 mpi Exp $	*/
/*	$NetBSD: cons.c,v 1.30 1996/04/08 19:57:30 jonathan Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1988 University of Utah.
 * Copyright (c) 1990, 1993
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
 *
 * from: Utah $Hdr: cons.h 1.6 92/01/21$
 * from: Utah $Hdr: cons.c 1.7 92/01/21$
 *
 *	@(#)cons.h	8.1 (Berkeley) 6/10/93
 *	@(#)cons.c	8.2 (Berkeley) 1/12/94
 */
/* </LICENSES> */

//! The console framework: `<dev/cons.h>` (the `struct consdev` contract and the `CN_*`
//! priorities) and `dev/cons.c` (the polled entry points `cngetc`, `cnputc`, `cnpollc`,
//! `cnbell` on top of `cn_tab`).
//!
//! Upstream: sys/dev/cons.h @ 3ce1f3f79392
//! Upstream: sys/dev/cons.c @ 3ce1f3f79392
//!
//! A console driver fills a [`Consdev`] with its polled routines and points `cn_tab` at it
//! (`comcnattach`, `pluartcnattach`). Everything the kernel prints, and everything `ddb(4)`
//! prints, goes through `cnputc` here. `/dev/console` (character major 0, `cnopen` ..
//! `cnkqfilter`) is the real console device's tty seen through `cn_tab->cn_dev`, or the
//! terminal a `TIOCCONS` redirected it to (`constty`).
//!
//! Status: `wip`.
//!
//! ## Deviations
//! - `cn_tab`, `constty` and `cn_devvp` are behind accessors ([`cn_tab`]/[`set_cn_tab`],
//!   [`constty`]/[`set_constty`]) over atomic pointers instead of globals.
//! - `cninit` (the `constab[]` probe loop) is not used: the architectures attach their
//!   console directly (`docs/ARCHITECTURE.md`, "Console attach").
//! - `cnpollc` takes a `bool`; its `int on` is only ever 0 or 1.

use core::cell::Cell;
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

use crate::kern::kern_prot::suser;
use crate::kern::vfs_subr::{cdevvp, vcount, vfinddev, vrele};
use crate::machine::conf::cdevsw;
use crate::sys::errno::Errno;
use crate::sys::event::Knote;
use crate::sys::param::NODEV;
use crate::sys::proc::Proc;
use crate::sys::tty::Tty;
use crate::sys::ttycom::TIOCCONS;
use crate::sys::types::{Dev, major};
use crate::sys::uio::Uio;
use crate::sys::vnode::{VCHR, Vnode};

/// Values for `cn_pri`: policy for console selection.
///
/// `CN_DEAD`: device does not exist.
pub const CN_DEAD: u32 = 0;
/// Device exists and is low priority.
pub const CN_LOWPRI: u32 = 1;
/// Device exists and is medium priority.
pub const CN_MIDPRI: u32 = 2;
/// Device exists and is high priority.
pub const CN_HIGHPRI: u32 = 3;
/// Use this device.
pub const CN_FORCED: u32 = 4;

/// `CONSMAJOR`: the major number of `/dev/console` (XXX in C too).
pub const CONSMAJOR: u32 = 0;

/// `struct consdev`: a console device's polled routines.
pub struct Consdev {
    /// Probe hardware and fill in consdev info.
    pub cn_probe: Option<fn(&Consdev)>,
    /// Turn on as console.
    pub cn_init: Option<fn(&Consdev)>,
    /// Kernel getchar interface.
    pub cn_getc: fn(Dev) -> i32,
    /// Kernel putchar interface.
    pub cn_putc: fn(Dev, i32),
    /// Turn on and off polling.
    pub cn_pollc: fn(Dev, bool),
    /// Ring bell.
    pub cn_bell: Option<fn(Dev, u32, u32, u32)>,
    /// Major/minor of device.
    pub cn_dev: Cell<Dev>,
    /// Picking order; the higher the better.
    pub cn_pri: Cell<u32>,
}

// SAFETY: `cn_dev` and `cn_pri` are written by the driver's attach or probe routine, on the boot
// CPU before interrupts exist, and only read afterwards; the function pointers never change.
unsafe impl Sync for Consdev {}

/// `cn_tab`: the console device, null until one attaches.
static CN_TAB: AtomicPtr<Consdev> = AtomicPtr::new(ptr::null_mut());

/// The attached console device, if any (`cn_tab`).
pub fn cn_tab() -> Option<&'static Consdev> {
    // SAFETY: the pointer is null or was stored by `set_cn_tab` from a `&'static Consdev`.
    unsafe { CN_TAB.load(Ordering::Acquire).as_ref() }
}

/// Makes `cp` the console device (`cn_tab = cp`).
pub fn set_cn_tab(cp: &'static Consdev) {
    CN_TAB.store(ptr::from_ref(cp).cast_mut(), Ordering::Release);
}

/// `constty`: virtual console output device, NULL when none.
static CONSTTY: AtomicPtr<Tty> = AtomicPtr::new(ptr::null_mut());

/// `cn_devvp`: vnode for underlying device, NULL when none.
static CN_DEVVP: AtomicPtr<Vnode> = AtomicPtr::new(ptr::null_mut());

/// `constty`: the terminal console output is redirected to (`TIOCCONS`), if any.
pub fn constty() -> Option<&'static Tty> {
    // SAFETY: null or a tty stored by `set_constty`; ttys live until `ttyfree`, which runs
    // after `ttyclose` cleared `constty`.
    unsafe { CONSTTY.load(Ordering::Acquire).as_ref() }
}

/// `constty = tp`.
pub fn set_constty(tp: Option<&Tty>) {
    CONSTTY.store(
        tp.map_or(ptr::null_mut(), |t| ptr::from_ref(t).cast_mut()),
        Ordering::Release,
    );
}

/// `cnopen`: always open the 'real' console device, so we don't get nailed later. This
/// follows normal device semantics; they always get open() calls.
pub fn cnopen(dev: Dev, flag: i32, mode: i32, p: &Proc) -> Result<(), Errno> {
    let Some(cp) = cn_tab() else {
        return Ok(());
    };

    let cndev = cp.cn_dev.get();
    if cndev == NODEV {
        return Err(Errno::ENXIO);
    }
    #[cfg(feature = "diagnostic")]
    if cndev == dev {
        crate::kern::subr_prf::panic(format_args!("cnopen: recursive"));
    }
    let _ = dev;
    if CN_DEVVP.load(Ordering::Relaxed).is_null() {
        // try to get a reference on its vnode, but fail silently
        if let Ok(Some(vp)) = cdevvp(cndev) {
            CN_DEVVP.store(ptr::from_ref(vp).cast_mut(), Ordering::Relaxed);
        }
    }
    (cdevsw(major(cndev)).d_open)(cndev, flag, mode, p)
}

/// `cnclose`: if the real console isn't otherwise open, close it. If it's otherwise open,
/// don't close it, because that'll screw up others who have it open.
pub fn cnclose(_dev: Dev, flag: i32, mode: i32, p: Option<&Proc>) -> Result<(), Errno> {
    let Some(cp) = cn_tab() else {
        return Ok(());
    };

    let dev = cp.cn_dev.get();
    let vp = CN_DEVVP.swap(ptr::null_mut(), Ordering::Relaxed);
    // SAFETY: null or the vnode `cnopen`'s `cdevvp` returned, holding the reference
    // released here; vnodes are never freed.
    if let Some(vp) = unsafe { vp.as_ref() } {
        // release our reference to real dev's vnode
        vrele(vp);
    }
    if vfinddev(dev, VCHR).is_some_and(|vp| vcount(vp) != 0) {
        return Ok(());
    }
    (cdevsw(major(dev)).d_close)(dev, flag, mode, p)
}

/// `cnread`: if we would redirect input, punt. This will keep strange things from happening
/// to people who are using the real console. Nothing should be using /dev/console for input
/// (except a shell in single-user mode, but then, one wouldn't TIOCCONS then).
pub fn cnread(_dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    if constty().is_some() {
        return Ok(());
    }
    let Some(cp) = cn_tab() else {
        return Err(Errno::ENXIO);
    };

    let dev = cp.cn_dev.get();
    (cdevsw(major(dev)).d_read)(dev, uio, flag)
}

/// `cnwrite`: redirect output, if that's appropriate. If there's no real console, return
/// `ENXIO`.
pub fn cnwrite(_dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let dev = if let Some(tp) = constty() {
        tp.t_dev.get()
    } else if let Some(cp) = cn_tab() {
        cp.cn_dev.get()
    } else {
        return Err(Errno::ENXIO);
    };
    (cdevsw(major(dev)).d_write)(dev, uio, flag)
}

/// `cnstop`.
pub fn cnstop(_tp: &Tty, _flag: i32) -> Result<(), Errno> {
    Ok(())
}

/// `cnioctl`.
pub fn cnioctl(_dev: Dev, cmd: u64, data: &mut [u8], flag: i32, p: &Proc) -> Result<(), Errno> {
    // Superuser can always use this to wrest control of console output from the "virtual"
    // console.
    if cmd == TIOCCONS && constty().is_some() {
        suser(p)?;
        set_constty(None);
        return Ok(());
    }

    // Redirect the ioctl, if that's appropriate. Note that strange things can happen, if a
    // program does ioctls on /dev/console, then the console is redirected out from under
    // it.
    let dev = if let Some(tp) = constty() {
        tp.t_dev.get()
    } else if let Some(cp) = cn_tab() {
        cp.cn_dev.get()
    } else {
        return Err(Errno::ENXIO);
    };
    (cdevsw(major(dev)).d_ioctl)(dev, cmd, data, flag, p)
}

/// `cnkqfilter`: redirect output, if that's appropriate. If there's no real console,
/// return `ENXIO`.
pub fn cnkqfilter(_dev: Dev, kn: &Knote) -> Result<(), Errno> {
    let dev = if let Some(tp) = constty() {
        tp.t_dev.get()
    } else if let Some(cp) = cn_tab() {
        cp.cn_dev.get()
    } else {
        return Err(Errno::ENXIO);
    };
    match cdevsw(major(dev)).d_kqfilter {
        Some(kqfilter) => kqfilter(dev, kn),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `cngetc`: reads one character from the console, blocking; 0 when there is no console.
pub fn cngetc() -> i32 {
    match cn_tab() {
        None => 0,
        Some(cp) => (cp.cn_getc)(cp.cn_dev.get()),
    }
}

/// `cnputc`: writes one character to the console, a `'\n'` followed by `'\r'`. A NUL and the
/// absence of a console are ignored.
pub fn cnputc(c: i32) {
    let Some(cp) = cn_tab() else {
        return;
    };
    if c != 0 {
        (cp.cn_putc)(cp.cn_dev.get(), c);
        if c == i32::from(b'\n') {
            (cp.cn_putc)(cp.cn_dev.get(), i32::from(b'\r'));
        }
    }
}

/// `cnpollc`: turns polling on or off, reference-counted so nested users agree.
pub fn cnpollc(on: bool) {
    static REFCOUNT: AtomicI32 = AtomicI32::new(0);

    let Some(cp) = cn_tab() else {
        return;
    };
    if !on {
        REFCOUNT.fetch_sub(1, Ordering::Relaxed);
    }
    if REFCOUNT.load(Ordering::Relaxed) == 0 {
        (cp.cn_pollc)(cp.cn_dev.get(), on);
    }
    if on {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
    }
}

/// `nullcnpollc`: the polling hook of a device that needs none.
pub fn nullcnpollc(_dev: Dev, _on: bool) {}

/// `cnbell`: rings the console bell, if the device has one.
pub fn cnbell(pitch: u32, period: u32, volume: u32) {
    if let Some(cp) = cn_tab()
        && let Some(bell) = cp.cn_bell
    {
        bell(cp.cn_dev.get(), pitch, period, volume);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::RefCell;
    use std::thread_local;
    use std::vec::Vec;

    // `cn_tab` is global, so a test in another thread may print through this device while
    // this one runs; recording per thread keeps each test's output its own.
    thread_local! {
        static OUT: RefCell<Vec<i32>> = const { RefCell::new(Vec::new()) };
        static POLL: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
    }

    fn getc(_dev: Dev) -> i32 {
        i32::from(b'x')
    }
    fn putc(_dev: Dev, c: i32) {
        OUT.with(|out| out.borrow_mut().push(c));
    }
    fn pollc(_dev: Dev, on: bool) {
        POLL.with(|poll| poll.borrow_mut().push(on));
    }

    static TESTCONS: Consdev = Consdev {
        cn_probe: None,
        cn_init: None,
        cn_getc: getc,
        cn_putc: putc,
        cn_pollc: pollc,
        cn_bell: None,
        cn_dev: Cell::new(NODEV),
        cn_pri: Cell::new(CN_LOWPRI),
    };

    #[test]
    fn newline_gets_a_carriage_return_and_polling_is_counted() {
        set_cn_tab(&TESTCONS);
        cnputc(0);
        cnputc(i32::from(b'a'));
        cnputc(i32::from(b'\n'));
        OUT.with(|out| assert_eq!(*out.borrow(), [i32::from(b'a'), 10, 13]));
        assert_eq!(cngetc(), i32::from(b'x'));
        cnpollc(true);
        cnpollc(true);
        cnpollc(false);
        cnpollc(false);
        POLL.with(|poll| assert_eq!(*poll.borrow(), [true, false]));
        cnbell(1, 2, 3);
    }
}
