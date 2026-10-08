/*	$OpenBSD: tty_tty.c,v 1.34 2025/04/17 12:01:26 jsg Exp $	*/
/*	$NetBSD: tty_tty.c,v 1.13 1996/03/30 22:24:46 christos Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1982, 1986, 1991, 1993
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
 *	@(#)tty_tty.c	8.2 (Berkeley) 9/23/93
 */
/* </LICENSES> */

/* <CODE> */
//! Indirect driver for controlling tty (`/dev/tty`): `kern/tty_tty.c`.
//!
//! Upstream: sys/kern/tty_tty.c @ 3ce1f3f79392
//!
//! Every call is passed to the vnode of the calling process's controlling terminal
//! (`s_ttyvp`, which `vn_ioctl`'s `TIOCSCTTY` records).
//!
//! ## Deviations
//! - `cttyvp(p)` is a function returning `Option<&'static Vnode>`.

use core::ffi::c_void;
use core::sync::atomic::Ordering;

use crate::kern::kern_event::seltrue_kqfilter;
use crate::kern::kern_proc::zapverauth;
use crate::kern::kern_prot::suser;
use crate::kern::kern_timeout::{timeout_add_sec, timeout_del};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_IOCTL, VOP_KQFILTER, VOP_OPEN, VOP_READ, VOP_UNLOCK, VOP_WRITE};
use crate::machine::cpu::curproc;
use crate::sys::errno::Errno;
use crate::sys::event::{__EV_POLL, __EV_SELECT, Knote};
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::ioctl::ioctl_arg;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::proc::{PS_CONTROLT, Proc, Session, sess_leader};
use crate::sys::ttycom::{TIOCCHKVERAUTH, TIOCCLRVERAUTH, TIOCNOTTY, TIOCSCTTY, TIOCSETVERAUTH};
use crate::sys::types::Dev;
use crate::sys::ucred::NOCRED;
use crate::sys::uio::Uio;
use crate::sys::vnode::Vnode;

/// `cttyvp(p)`: the vnode of `p`'s controlling terminal, if it has one.
fn cttyvp(p: &Proc) -> Option<&'static Vnode> {
    let pr = p.process();
    if pr.ps_flags.load(Ordering::Relaxed) & PS_CONTROLT == 0 {
        return None;
    }
    // SAFETY: a process's session lives while the process is in it; `s_ttyvp` holds a use
    // count on its vnode, and vnodes are never freed.
    unsafe { pr.session().as_ref()?.s_ttyvp.get().as_ref() }
}

/// The session of `p`'s process group (`p->p_p->ps_pgrp->pg_session`).
fn pgrp_session(p: &Proc) -> Option<&'static Session> {
    // SAFETY: as for `cttyvp`.
    unsafe { p.process().session().as_ref() }
}

/// `cttyopen`.
pub fn cttyopen(_dev: Dev, flag: i32, _mode: i32, p: &Proc) -> Result<(), Errno> {
    let Some(ttyvp) = cttyvp(p) else {
        return Err(Errno::ENXIO);
    };
    let _ = vn_lock(ttyvp, LK_EXCLUSIVE | LK_RETRY);
    let error = VOP_OPEN(ttyvp, flag, NOCRED, p);
    let _ = VOP_UNLOCK(ttyvp);
    error
}

/// `cttyread`.
pub fn cttyread(_dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let Some(ttyvp) = uio.uio_procp.and_then(cttyvp) else {
        return Err(Errno::EIO);
    };
    let _ = vn_lock(ttyvp, LK_EXCLUSIVE | LK_RETRY);
    let error = VOP_READ(ttyvp, uio, flag, NOCRED);
    let _ = VOP_UNLOCK(ttyvp);
    error
}

/// `cttywrite`.
pub fn cttywrite(_dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let Some(ttyvp) = uio.uio_procp.and_then(cttyvp) else {
        return Err(Errno::EIO);
    };
    let _ = vn_lock(ttyvp, LK_EXCLUSIVE | LK_RETRY);
    let error = VOP_WRITE(ttyvp, uio, flag, NOCRED);
    let _ = VOP_UNLOCK(ttyvp);
    error
}

/// `cttyioctl`.
pub fn cttyioctl(_dev: Dev, cmd: u64, addr: &mut [u8], flag: i32, p: &Proc) -> Result<(), Errno> {
    let Some(ttyvp) = cttyvp(p) else {
        return Err(Errno::EIO);
    };
    match cmd {
        TIOCSCTTY => {
            // XXX
            return Err(Errno::EINVAL);
        }
        TIOCNOTTY => {
            if !sess_leader(p.process()) {
                p.process()
                    .ps_flags
                    .fetch_and(!PS_CONTROLT, Ordering::SeqCst);
                return Ok(());
            } else {
                return Err(Errno::EINVAL);
            }
        }
        TIOCSETVERAUTH => {
            suser(p)?;
            let secs = ioctl_arg::<i32>(addr);
            if !(1..=3600).contains(&secs) {
                return Err(Errno::EINVAL);
            }
            let Some(sess) = pgrp_session(p) else {
                return Err(Errno::EINVAL);
            };
            sess.s_verauthuid.set(p.ucred().cr_ruid.get());
            sess.s_verauthppid.set(p.process().ps_ppid.get());
            timeout_add_sec(&sess.s_verauthto, secs);
            return Ok(());
        }
        TIOCCLRVERAUTH => {
            if let Some(sess) = pgrp_session(p) {
                timeout_del(&sess.s_verauthto);
                zapverauth(core::ptr::from_ref(sess).cast_mut().cast::<c_void>());
            }
            return Ok(());
        }
        TIOCCHKVERAUTH => {
            // It's not clear when or what these checks are for. How can we reach this code
            // with a different ruid? The ppid check is also more porous than desired.
            // Nevertheless, the checks reflect the original intention; namely, that it be
            // the same user using the same shell.
            if let Some(sess) = pgrp_session(p)
                && sess.s_verauthuid.get() == p.ucred().cr_ruid.get()
                && sess.s_verauthppid.get() == p.process().ps_ppid.get()
            {
                return Ok(());
            }
            return Err(Errno::EPERM);
        }
        _ => {}
    }
    VOP_IOCTL(ttyvp, cmd, addr, flag, NOCRED, p)
}

/// `cttykqfilter`: the controlling terminal's `VOP_KQFILTER`; without one, poll and select
/// see an always-ready device and `kevent(2)` gets `ENXIO`.
pub fn cttykqfilter(dev: Dev, kn: &Knote) -> Result<(), Errno> {
    let Some(ttyvp) = curproc().and_then(cttyvp) else {
        if kn.has_flags(__EV_POLL | __EV_SELECT) {
            return seltrue_kqfilter(dev, kn);
        }
        return Err(Errno::ENXIO);
    };
    VOP_KQFILTER(ttyvp, FREAD | FWRITE, kn)
}
/* </CODE> */
