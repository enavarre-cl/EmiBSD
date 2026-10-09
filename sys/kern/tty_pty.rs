/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1993
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
 *	@(#)tty_pty.c	8.4 (Berkeley) 2/20/95
 */
/* </LICENSES> */

/* <CODE> */
//! The pseudo-teletype driver (actually two drivers, requiring two entries in `cdevsw`),
//! and the `/dev/ptm` cloning device: `kern/tty_pty.c`.
//!
//! Upstream: sys/kern/tty_pty.c @ 3ce1f3f79392
//!
//! `pts` is `/dev/tty[p-zP-T][0-9a-zA-Z]`, the slave side a program uses as its terminal;
//! `ptc` is `/dev/pty[p-zP-T][0-9a-zA-Z]`, the master side (`sshd`, `xterm`, `script`)
//! that sees the slave's output and feeds its input. `ptm`'s `PTMGET` opens a free pair.
//!
//! ## Deviations
//! - `pt_softc[]` (grown by powers of two under `pt_softc_lock`) is an array of
//!   `Option<&'static PtSoftc>` from `malloc`, reached through [`pt_softc`], which answers
//!   `None` past `npty` where the C would index out of bounds; the entry points answer
//!   `ENXIO` then. A softc and its tty are never freed, as in C.
//! - `ptsclose` ORs the errnos of the line discipline's close and `ttyclose` in C; here the
//!   first error wins.
//! - `ptsread`'s remote mode tests `ureadc(..) < 0`, which no errno is, so the C ignores a
//!   copy error there; so does this port.
//! - `ptckqfilter` answers `ENXIO` for a minor without a softc, where the C would
//!   dereference NULL; the filters reach the softc through `kn_hook` ([`kn_pti`]).
//! - `ptyioctl` recognises the master by comparing `cdevsw[major(dev)].d_open` with
//!   `ptcopen` as the C does, with `ptr::fn_addr_eq` (`docs/C_TO_RUST.md`).
//! - `PTMGET` opens `/dev/ptyXX` and `/dev/ttyXX` through `namei`, as the C does; without a
//!   root file system both lookups fail with `ENOENT`.
//! - `NPTY` (`pty.h`, which `config(8)` writes from `pseudo-device pty 16`) is a constant
//!   here.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::kern::kern_descrip::{closef, falloc, fdinsert, fdremove};
use crate::kern::kern_event::{klist_insert_locked, klist_remove_locked};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_prot::{crfree, crget, suser};
use crate::kern::kern_rwlock::{rw_enter_read, rw_enter_write, rw_exit_read, rw_exit_write};
use crate::kern::kern_sig::{pgsignal, sigismasked};
use crate::kern::kern_subr::{uiomove, ureadc};
use crate::kern::kern_synch::{nowake, tsleep, tsleep_nsec, wakeup};
use crate::kern::kern_tc::getnanotime;
use crate::kern::subr_prf::{panic, tablefull};
use crate::kern::sys_generic::selwakeup;
use crate::kern::tty::ttymalloc;
use crate::kern::tty::{
    TTOPEN, TTYBG, TTYIN, TTYOUT, ttioctl, ttsetwater, ttwakeup, ttwakeupwr, ttychars, ttyclose,
    ttyflush, ttyinfo, ttysleep, ttysleep_nsec,
};
use crate::kern::tty_conf::linesw;
use crate::kern::tty_subr::{b_to_q, getc, ndflush, putc, q_to_b};
use crate::kern::vfs_lookup::{namei, ndinit};
use crate::kern::vfs_subr::{vattr_null, vput, vrele};
use crate::kern::vfs_vnops::VNOPS;
use crate::kern::vfs_vops::{VOP_OPEN, VOP_REVOKE, VOP_SETATTR, VOP_UNLOCK};
use crate::machine::conf::{cdevsw, nchrdev};
use crate::machine::cpu::curproc;
use crate::machine::intr::{spltty, splx};
use crate::sys::conf::DevTypeOpen;
use crate::sys::errno::Errno;
use crate::sys::event::{
    __EV_HUP, __EV_POLL, __EV_SELECT, EV_EOF, EVFILT_EXCEPT, EVFILT_READ, EVFILT_WRITE,
    FILTEROP_ISFD, Filterops, Knote, NOTE_OOB,
};
use crate::sys::fcntl::{FNONBLOCK, FREAD, FWRITE};
use crate::sys::file::{DTYPE_VNODE, frele};
use crate::sys::filedesc::{fdplock, fdpunlock};
use crate::sys::filio::FIONREAD;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::mount::MNT_RDONLY;
use crate::sys::namei::{KERNELPATH, LOCKLEAF, LOOKUP, NOFOLLOW, Nameidata, NiDirp};
use crate::sys::param::PCATCH;
use crate::sys::pledge::{PLEDGE_RPATH, PLEDGE_WPATH};
use crate::sys::proc::{PS_PPWAIT, Proc};
use crate::sys::rwlock::Rwlock;
use crate::sys::selinfo::Selinfo;
use crate::sys::signal::{NSIG, SIGINFO, SIGTTIN};
use crate::sys::stat::{ALLPERMS, S_IRUSR, S_IWGRP, S_IWUSR};
use crate::sys::termios::{
    B115200, EXTPROC, ICANON, IXON, NOFLSH, NOKERNINFO, Termios, VSTART, VSTOP, cceq,
};
use crate::sys::time::sec_to_nsec;
use crate::sys::tty::{
    PTMGET, Ptmget, TS_CARR_ON, TS_ISOPEN, TS_TTSTOP, TS_WOPEN, TS_XCLUDE, TTIPRI, TTOPRI, TTY_GID,
    Tty, isbackground, ttyhog,
};
use crate::sys::ttycom::{
    TIOCCBRK, TIOCEXT, TIOCGPGRP, TIOCPKT, TIOCPKT_DOSTOP, TIOCPKT_IOCTL, TIOCPKT_NOSTOP,
    TIOCPKT_START, TIOCPKT_STOP, TIOCREMOTE, TIOCSBRK, TIOCSETA, TIOCSETAF, TIOCSETAW, TIOCSETD,
    TIOCSIG, TIOCUCNTL, TIOCUCNTL_CBRK, TIOCUCNTL_SBRK, uioccmd,
};
use crate::sys::ttydefaults::{TTYDEF_CFLAG, TTYDEF_IFLAG, TTYDEF_LFLAG, TTYDEF_OFLAG, ctrl};
use crate::sys::types::{Dev, major, makedev, minor};
use crate::sys::uio::Uio;
use crate::sys::vnode::{IO_NDELAY, REVOKEALL, VA_UTIMES_NULL, VALIASED, VCHR, Vattr};

/// Chunk size iomoved to/from user.
const BUFSIZ: usize = 100;

// XXX this needs to come from somewhere sane, and work with MAKEDEV

/// The letters of the pty names.
const TTY_LETTERS: &[u8] = b"pqrstuvwxyzPQRST";
/// The suffixes of the pty names.
const TTY_SUFFIX: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// `NPTY`: `pseudo-device pty 16` (MI `conf/GENERIC`).
pub const NPTY: i32 = 16;

/// Number of initial ptys.
pub const NPTY_MIN: i32 = 8;
/// Maximum number of ptys supported.
pub const NPTY_MAX: i32 = 992;

/// Packet mode.
pub const PF_PKT: i32 = 0x08;
/// User told stopped.
pub const PF_STOPPED: i32 = 0x10;
/// Remote and flow controlled input.
pub const PF_REMOTE: i32 = 0x20;
/// No ^S/^Q flow control.
pub const PF_NOSTOP: i32 = 0x40;
/// User control mode.
pub const PF_UCNTL: i32 = 0x80;

/// `struct pt_softc`: one pseudo-terminal.
pub struct PtSoftc {
    /// `pt_tty`.
    pub pt_tty: &'static Tty,
    /// `pt_flags` (`PF_*`).
    pub pt_flags: Cell<i32>,
    /// `pt_selr`.
    pub pt_selr: Selinfo,
    /// `pt_selw`.
    pub pt_selw: Selinfo,
    /// `pt_send`: the `TIOCPKT_*` byte the master reads next in packet mode.
    pub pt_send: Cell<u8>,
    /// `pt_ucntl`: the user control byte the master reads next.
    pub pt_ucntl: Cell<u8>,
    /// `pty_pn`: the master's path, NUL-terminated.
    pub pty_pn: Cell<[u8; 11]>,
    /// `pty_sn`: the slave's path, NUL-terminated.
    pub pty_sn: Cell<[u8; 11]>,
}

// SAFETY: changed at spltty under the kernel lock, as the tty is.
unsafe impl Sync for PtSoftc {}

/// The pty array: `pt_softc` and `npty`.
struct PtArray {
    /// `pt_softc`: `npty` slots.
    slots: Cell<*mut Option<&'static PtSoftc>>,
    /// `npty`: size of pty array.
    npty: Cell<i32>,
}

// SAFETY: grown and filled under `pt_softc_lock`, read by the entry points of devices
// `check_pty` already made, under the kernel lock.
unsafe impl Sync for PtArray {}

/// `pt_softc` and `npty`.
static PT: PtArray = PtArray {
    slots: Cell::new(ptr::null_mut()),
    npty: Cell::new(0),
};

/// `pts_major`.
static PTS_MAJOR: AtomicI32 = AtomicI32::new(0);

/// `maxptys`: maximum number of ptys.
static MAXPTYS: i32 = NPTY_MAX;

/// `pt_softc_lock`: for pty array.
pub static PT_SOFTC_LOCK: Rwlock = Rwlock::new("ptarrlk");

/// `tty_gid`.
static TTY_GID_: u32 = TTY_GID;

/// `ptcread_filtops`.
pub static PTCREAD_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_ptcrdetach),
    f_event: Some(filt_ptcread),
    f_modify: None,
    f_process: None,
};

/// `ptcwrite_filtops`.
pub static PTCWRITE_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_ptcwdetach),
    f_event: Some(filt_ptcwrite),
    f_modify: None,
    f_process: None,
};

/// `ptcexcept_filtops`.
pub static PTCEXCEPT_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_ptcrdetach),
    f_event: Some(filt_ptcexcept),
    f_modify: None,
    f_process: None,
};

/// `pt_softc[minor]`, `None` for a slot not made yet.
pub fn pt_softc(minor: u32) -> Option<&'static PtSoftc> {
    if minor as i32 >= PT.npty.get() {
        return None;
    }
    // SAFETY: `slots` has `npty` initialised slots (`ptyarralloc` zeroes them, `check_pty`
    // fills them); `minor < npty`.
    unsafe { *PT.slots.get().add(minor as usize) }
}

/// The softc and tty of `dev`, for the entry points (`ENXIO` for a slot never made).
fn pty_of(dev: Dev) -> Result<(&'static PtSoftc, &'static Tty), Errno> {
    let pti = pt_softc(minor(dev)).ok_or(Errno::ENXIO)?;
    Ok((pti, pti.pt_tty))
}

/// `ptydevname`: the names of pty `minor`.
pub fn ptydevname(minor: i32, pti: &PtSoftc) {
    let mut buf = *b"/dev/XtyXX\0";

    let i = minor as usize / TTY_SUFFIX.len();
    let j = minor as usize % TTY_SUFFIX.len();
    if i >= TTY_LETTERS.len() {
        pti.pty_pn.set([0; 11]);
        pti.pty_sn.set([0; 11]);
        return;
    }
    buf[5] = b'p';
    buf[8] = TTY_LETTERS[i];
    buf[9] = TTY_SUFFIX[j];
    pti.pty_pn.set(buf);
    buf[5] = b't';
    pti.pty_sn.set(buf);
}

/// `ptyarralloc`: allocate and zero array of `nelem` elements.
fn ptyarralloc(nelem: i32) -> *mut Option<&'static PtSoftc> {
    let size = nelem as usize * size_of::<Option<&'static PtSoftc>>();
    let Some(pt) = malloc(size, M_DEVBUF, M_WAITOK | M_ZERO) else {
        panic(format_args!("ptyarralloc: out of memory"));
    };
    // All-zero is `None` for every slot.
    pt.as_ptr().cast()
}

/// `check_pty`: check if the minor is correct and ensure necessary structures are properly
/// allocated.
pub fn check_pty(dev: Dev) -> Result<(), Errno> {
    let minor = minor(dev) as i32;

    rw_enter_write(&PT_SOFTC_LOCK);
    if minor >= PT.npty.get() {
        // check if the requested pty can be granted
        if minor >= MAXPTYS {
            rw_exit_write(&PT_SOFTC_LOCK);
            tablefull("pty");
            return Err(Errno::ENXIO);
        }

        // grow pty array by powers of two, up to maxptys
        let mut newnpty = PT.npty.get().max(1);
        while newnpty <= minor {
            newnpty *= 2;
        }

        if newnpty > MAXPTYS {
            newnpty = MAXPTYS;
        }
        let newpt = ptyarralloc(newnpty);

        let old = PT.slots.get();
        let npty = PT.npty.get() as usize;
        if !old.is_null() {
            // SAFETY: both arrays are `malloc`ed slot arrays, the old one with `npty` slots,
            // the new one with more; they do not overlap.
            unsafe { ptr::copy_nonoverlapping(old, newpt, npty) };
            if let Some(old) = NonNull::new(old) {
                free(
                    old.cast::<u8>(),
                    M_DEVBUF,
                    npty * size_of::<Option<&'static PtSoftc>>(),
                );
            }
        }
        PT.slots.set(newpt);
        PT.npty.set(newnpty);
    }

    // If the entry is not yet allocated, allocate one.
    if pt_softc(minor as u32).is_none() {
        let Some(mem) = malloc(size_of::<PtSoftc>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
            panic(format_args!("check_pty: out of memory"));
        };
        let tp = ttymalloc(1_000_000);
        tp.t_dev.set(dev);
        let pti = mem.cast::<PtSoftc>();
        // SAFETY: a fresh, suitably aligned allocation of `size_of::<PtSoftc>()` bytes,
        // written once before it is published; it is never freed.
        let pti: &'static PtSoftc = unsafe {
            pti.as_ptr().write(PtSoftc {
                pt_tty: tp,
                pt_flags: Cell::new(0),
                pt_selr: Selinfo::new(),
                pt_selw: Selinfo::new(),
                pt_send: Cell::new(0),
                pt_ucntl: Cell::new(0),
                pty_pn: Cell::new([0; 11]),
                pty_sn: Cell::new([0; 11]),
            });
            pti.as_ref()
        };
        ptydevname(minor, pti);
        // SAFETY: `minor < npty`, a slot of the array, under pt_softc_lock.
        unsafe { *PT.slots.get().add(minor as usize) = Some(pti) };
    }
    rw_exit_write(&PT_SOFTC_LOCK);
    Ok(())
}

/// `ptyattach`: establish `n` (or default if `n` is 1) ptys in the system.
pub fn ptyattach(n: i32) {
    // maybe should allow 0 => none?
    let n = if n <= 1 { NPTY_MIN } else { n };
    PT.slots.set(ptyarralloc(n));
    PT.npty.set(n);

    // If we have pty, we need ptm too.
    ptmattach(1);
}

/// `ptsopen`: open the slave side.
pub fn ptsopen(dev: Dev, flag: i32, _devtype: i32, p: &Proc) -> Result<(), Errno> {
    check_pty(dev)?;

    let (_pti, tp) = pty_of(dev)?;
    if !tp.t_state_isset(TS_ISOPEN) {
        tp.t_state_set(TS_WOPEN);
        ttychars(tp); // Set up default chars
        tp.set_t_iflag(TTYDEF_IFLAG);
        tp.set_t_oflag(TTYDEF_OFLAG);
        tp.set_t_lflag(TTYDEF_LFLAG);
        tp.set_t_cflag(TTYDEF_CFLAG);
        tp.set_t_ispeed(B115200 as i32);
        tp.set_t_ospeed(B115200 as i32);
        ttsetwater(tp); // would be done in xxparam()
    } else if tp.t_state_isset(TS_XCLUDE) && suser(p).is_err() {
        return Err(Errno::EBUSY);
    }
    if tp.t_oproc.get().is_some() {
        // Ctrlr still around.
        tp.t_state_set(TS_CARR_ON);
    }
    while !tp.t_state_isset(TS_CARR_ON) {
        tp.t_state_set(TS_WOPEN);
        if flag & FNONBLOCK != 0 {
            break;
        }
        ttysleep(
            tp,
            ptr::from_ref(&tp.t_rawq).cast(),
            TTIPRI | PCATCH,
            TTOPEN,
        )?;
    }
    let error = (linesw(tp).l_open)(dev, tp, p);
    ptcwakeup(tp, FREAD | FWRITE);
    error
}

/// `ptsclose`: close the slave side.
pub fn ptsclose(dev: Dev, flag: i32, _mode: i32, p: Option<&Proc>) -> Result<(), Errno> {
    let (_pti, tp) = pty_of(dev)?;

    let error = (linesw(tp).l_close)(tp, flag, p);
    let error = error.and(ttyclose(tp));
    ptcwakeup(tp, FREAD | FWRITE);
    error
}

/// `ptsread`: read from the slave side.
pub fn ptsread(dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let (pti, tp) = pty_of(dev)?;
    let mut error = Ok(());

    loop {
        // again:
        if pti.pt_flags.get() & PF_REMOTE != 0 {
            if let Some(p) = curproc() {
                let pr = p.process();
                while isbackground(pr, tp) {
                    // SAFETY: a process in the background of a tty is in a process group.
                    let pgrp = unsafe { &*pr.ps_pgrp.get() };
                    if sigismasked(p, SIGTTIN)
                        || pgrp.pg_jobc.get() == 0
                        || pr.ps_flags.load(Ordering::Relaxed) & PS_PPWAIT != 0
                    {
                        return Err(Errno::EIO);
                    }
                    pgsignal(Some(pgrp), SIGTTIN, true);
                    ttysleep_nsec(tp, nowake(), TTIPRI | PCATCH, TTYBG, sec_to_nsec(1))?;
                }
            }
            if tp.t_canq.c_cc.get() == 0 {
                if flag & IO_NDELAY != 0 {
                    return Err(Errno::EWOULDBLOCK);
                }
                ttysleep(tp, ptr::from_ref(&tp.t_canq).cast(), TTIPRI | PCATCH, TTYIN)?;
                continue;
            }
            while tp.t_canq.c_cc.get() > 1 && uio.uio_resid > 0 {
                // The C's `ureadc(..) < 0` is never true (see the module's deviations).
                let _ = ureadc(getc(&tp.t_canq), uio);
            }
            if tp.t_canq.c_cc.get() == 1 {
                let _ = getc(&tp.t_canq);
            }
            if tp.t_canq.c_cc.get() != 0 {
                return error;
            }
        } else if tp.t_oproc.get().is_some() {
            error = (linesw(tp).l_read)(tp, uio, flag);
        }
        break;
    }
    ptcwakeup(tp, FWRITE);
    error
}

/// `ptswrite`: write to pseudo-tty. Wakeups of controlling tty will happen indirectly, when
/// tty driver calls `ptsstart`.
pub fn ptswrite(dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let (_pti, tp) = pty_of(dev)?;

    if tp.t_oproc.get().is_none() {
        return Err(Errno::EIO);
    }
    (linesw(tp).l_write)(tp, uio, flag)
}

/// `ptsstart`: start output on pseudo-tty. Wake up process polling or sleeping for input
/// from controlling tty.
pub fn ptsstart(tp: &Tty) {
    let Some(pti) = pt_softc(minor(tp.t_dev.get())) else {
        return;
    };

    if tp.t_state_isset(TS_TTSTOP) {
        return;
    }
    if pti.pt_flags.get() & PF_STOPPED != 0 {
        pti.pt_flags.set(pti.pt_flags.get() & !PF_STOPPED);
        pti.pt_send.set(TIOCPKT_START as u8);
    }
    ptcwakeup(tp, FREAD);
}

/// `ptsstop`: stop output on the slave side (the `d_stop` of `pts`).
pub fn ptsstop(tp: &Tty, flush: i32) -> Result<(), Errno> {
    let Some(pti) = pt_softc(minor(tp.t_dev.get())) else {
        return Ok(());
    };
    let mut flush = flush;

    // note: FLUSHREAD and FLUSHWRITE already ok
    if flush == 0 {
        flush = TIOCPKT_STOP;
        pti.pt_flags.set(pti.pt_flags.get() | PF_STOPPED);
    } else {
        pti.pt_flags.set(pti.pt_flags.get() & !PF_STOPPED);
    }
    pti.pt_send.set(pti.pt_send.get() | flush as u8);
    // change of perspective
    let mut flag = 0;
    if flush & FREAD != 0 {
        flag |= FWRITE;
    }
    if flush & FWRITE != 0 {
        flag |= FREAD;
    }
    ptcwakeup(tp, flag);
    Ok(())
}

/// `ptcwakeup`: wake up the master's readers (`FREAD`) or writers (`FWRITE`).
pub fn ptcwakeup(tp: &Tty, flag: i32) {
    let Some(pti) = pt_softc(minor(tp.t_dev.get())) else {
        return;
    };

    if flag & FREAD != 0 {
        selwakeup(&pti.pt_selr);
        wakeup(ptr::from_ref(&tp.t_outq.c_cf));
    }
    if flag & FWRITE != 0 {
        selwakeup(&pti.pt_selw);
        wakeup(ptr::from_ref(&tp.t_rawq.c_cf));
    }
}

/// `ptcopen`: open the master side.
pub fn ptcopen(dev: Dev, _flag: i32, _devtype: i32, _p: &Proc) -> Result<(), Errno> {
    check_pty(dev)?;

    let (pti, tp) = pty_of(dev)?;
    if tp.t_oproc.get().is_some() {
        return Err(Errno::EIO);
    }
    tp.t_oproc.set(Some(ptsstart));
    let _ = (linesw(tp).l_modem)(tp, 1);
    tp.t_lflag_clr(EXTPROC);
    pti.pt_flags.set(0);
    pti.pt_send.set(0);
    pti.pt_ucntl.set(0);
    Ok(())
}

/// `ptcclose`: close the master side.
pub fn ptcclose(dev: Dev, _flag: i32, _devtype: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let (_pti, tp) = pty_of(dev)?;

    let _ = (linesw(tp).l_modem)(tp, 0);
    tp.t_state_clr(TS_CARR_ON);
    tp.t_oproc.set(None); // mark closed
    Ok(())
}

/// `ptcread`: read the slave's output from the master side.
pub fn ptcread(dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let (pti, tp) = pty_of(dev)?;
    let mut buf = [0u8; BUFSIZ];
    let mut bufcc = 0;

    // We want to block until the slave is open, and there's something to read; but if we
    // lost the slave or we're NBIO, then return the appropriate error instead.
    loop {
        if tp.t_state_isset(TS_ISOPEN) {
            if pti.pt_flags.get() & PF_PKT != 0 && pti.pt_send.get() != 0 {
                ureadc(i32::from(pti.pt_send.get()), uio)?;
                if i32::from(pti.pt_send.get()) & TIOCPKT_IOCTL != 0 {
                    let cc = uio.uio_resid.min(size_of::<Termios>());
                    let mut t = [0u8; size_of::<Termios>()];
                    ioctl_ret(&mut t, &tp.t_termios.get());
                    uiomove(&mut t[..cc], uio)?;
                }
                pti.pt_send.set(0);
                return Ok(());
            }
            if pti.pt_flags.get() & PF_UCNTL != 0 && pti.pt_ucntl.get() != 0 {
                ureadc(i32::from(pti.pt_ucntl.get()), uio)?;
                pti.pt_ucntl.set(0);
                return Ok(());
            }
            if tp.t_outq.c_cc.get() != 0 && !tp.t_state_isset(TS_TTSTOP) {
                break;
            }
        }
        if !tp.t_state_isset(TS_CARR_ON) {
            return Ok(()); // EOF
        }
        if flag & IO_NDELAY != 0 {
            return Err(Errno::EWOULDBLOCK);
        }
        tsleep_nsec(
            ptr::from_ref(&tp.t_outq.c_cf),
            TTIPRI | PCATCH,
            TTYIN,
            crate::sys::systm::INFSLP,
        )?;
    }
    let mut error = Ok(());
    if pti.pt_flags.get() & (PF_PKT | PF_UCNTL) != 0 {
        error = ureadc(0, uio);
    }
    while uio.uio_resid > 0 && error.is_ok() {
        let cc = uio.uio_resid.min(BUFSIZ);
        let cc = q_to_b(&tp.t_outq, &mut buf[..cc]);
        if cc > bufcc {
            bufcc = cc;
        }
        if cc == 0 {
            break;
        }
        error = uiomove(&mut buf[..cc], uio);
    }
    ttwakeupwr(tp);
    if bufcc != 0 {
        libkern::explicit_bzero(&mut buf[..bufcc]);
    }
    error
}

/// How `ptcwrite` ends.
enum PtcExit {
    /// `goto block`.
    Block,
    /// `goto interrupt`.
    Interrupt,
    /// `goto done` with this result.
    Done(Result<(), Errno>),
}

/// `ptcwrite`: feed the slave's input from the master side.
pub fn ptcwrite(dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let (pti, tp) = pty_of(dev)?;
    let mut buf = [0u8; BUFSIZ];
    let mut cp = 0usize;
    let mut cc = 0usize;
    let mut bufcc = 0usize;
    let mut cnt = 0usize;

    let error = 'done: loop {
        // again:
        let exit = 'body: {
            if !tp.t_state_isset(TS_ISOPEN) {
                break 'body PtcExit::Block;
            }
            if pti.pt_flags.get() & PF_REMOTE != 0 {
                if tp.t_canq.c_cc.get() != 0 {
                    break 'body PtcExit::Block;
                }
                while uio.uio_resid > 0 && tp.t_canq.c_cc.get() < ttyhog(tp) - 1 {
                    if cc == 0 {
                        cc = uio.uio_resid.min(BUFSIZ);
                        cc = cc.min((ttyhog(tp) - 1 - tp.t_canq.c_cc.get()) as usize);
                        if cc > bufcc {
                            bufcc = cc;
                        }
                        cp = 0;
                        if let Err(e) = uiomove(&mut buf[..cc], uio) {
                            break 'body PtcExit::Done(Err(e));
                        }
                        // check again for safety
                        if !tp.t_state_isset(TS_ISOPEN) {
                            break 'body PtcExit::Done(Err(Errno::EIO));
                        }
                    }
                    if cc != 0 {
                        let _ = b_to_q(&buf[cp..cp + cc], &tp.t_canq);
                    }
                    cc = 0;
                }
                let _ = putc(0, &tp.t_canq);
                ttwakeup(tp);
                wakeup(ptr::from_ref(&tp.t_canq));
                break 'body PtcExit::Done(Ok(()));
            }
            loop {
                if cc == 0 {
                    cc = uio.uio_resid.min(BUFSIZ);
                    if cc > bufcc {
                        bufcc = cc;
                    }
                    cp = 0;
                    if let Err(e) = uiomove(&mut buf[..cc], uio) {
                        break 'body PtcExit::Done(Err(e));
                    }
                    // check again for safety
                    if !tp.t_state_isset(TS_ISOPEN) {
                        break 'body PtcExit::Done(Err(Errno::EIO));
                    }
                }
                bufcc = cc;
                while cc > 0 {
                    if tp.t_rawq.c_cc.get() + tp.t_canq.c_cc.get() >= ttyhog(tp) - 2
                        && (tp.t_canq.c_cc.get() > 0 || tp.t_lflag() & ICANON == 0)
                    {
                        wakeup(ptr::from_ref(&tp.t_rawq));
                        break 'body PtcExit::Block;
                    }
                    let c = i32::from(buf[cp]);
                    cp += 1;
                    if (linesw(tp).l_rint)(c, tp) == 1
                        && tsleep(ptr::from_ref(tp), TTIPRI | PCATCH, "ttyretype", 1)
                            == Err(Errno::EINTR)
                    {
                        break 'body PtcExit::Interrupt;
                    }
                    cnt += 1;
                    cc -= 1;
                }
                cc = 0;
                if uio.uio_resid == 0 {
                    break;
                }
            }
            PtcExit::Done(Ok(()))
        };

        match exit {
            PtcExit::Done(e) => break 'done e,
            PtcExit::Interrupt => {
                // adjust for data copied in but not written
                uio.uio_resid += cc;
                break 'done Ok(());
            }
            PtcExit::Block => {
                // Come here to wait for slave to open, for space in outq, or space in rawq.
                if !tp.t_state_isset(TS_CARR_ON) {
                    break 'done Err(Errno::EIO);
                }
                if flag & IO_NDELAY != 0 {
                    // adjust for data copied in but not written
                    uio.uio_resid += cc;
                    break 'done if cnt == 0 {
                        Err(Errno::EWOULDBLOCK)
                    } else {
                        Ok(())
                    };
                }
                if let Err(e) = tsleep_nsec(
                    ptr::from_ref(&tp.t_rawq.c_cf),
                    TTOPRI | PCATCH,
                    TTYOUT,
                    crate::sys::systm::INFSLP,
                ) {
                    // interrupt: adjust for data copied in but not written
                    uio.uio_resid += cc;
                    break 'done Err(e);
                }
            }
        }
    };
    // done:
    if bufcc != 0 {
        libkern::explicit_bzero(&mut buf[..bufcc]);
    }
    error
}

/// `kn->kn_hook` of a master-side knote: its pty.
pub fn kn_pti(kn: &Knote) -> &PtSoftc {
    // SAFETY: `ptckqfilter` points `kn_hook` at a softc, and softcs are never freed (the
    // array only grows).
    match unsafe { kn.kn_hook.get().cast::<PtSoftc>().as_ref() } {
        Some(pti) => pti,
        None => panic(format_args!("knote {:p}: no pty", kn)),
    }
}

/// `filt_ptcrdetach`: unhooks a read or except knote.
pub fn filt_ptcrdetach(kn: &Knote) {
    let pti = kn_pti(kn);

    let s = spltty();
    klist_remove_locked(&pti.pt_selr.si_note, kn);
    splx(s);
}

/// `filt_ptcread`: the master is readable when the slave has output (or a packet-mode or
/// user-control byte) for it; EOF (and, for poll, a hang-up) once the carrier is gone.
pub fn filt_ptcread(kn: &Knote, _hint: i64) -> bool {
    let pti = kn_pti(kn);
    let tp = pti.pt_tty;
    kn.kn_data().set(0);

    if tp.t_state_isset(TS_ISOPEN) {
        if !tp.t_state_isset(TS_TTSTOP) {
            kn.kn_data().set(i64::from(tp.t_outq.c_cc.get()));
        }
        if (pti.pt_flags.get() & PF_PKT != 0 && pti.pt_send.get() != 0)
            || (pti.pt_flags.get() & PF_UCNTL != 0 && pti.pt_ucntl.get() != 0)
        {
            kn.kn_data().set(kn.kn_data().get() + 1);
        }
    }
    let mut active = kn.kn_data().get() > 0;

    if !tp.t_state_isset(TS_CARR_ON) {
        kn.set_flags(EV_EOF);
        if kn.has_flags(__EV_POLL) {
            kn.set_flags(__EV_HUP);
        }
        active = true;
    } else {
        kn.clear_flags(EV_EOF | __EV_HUP);
    }

    active
}

/// `filt_ptcwdetach`: unhooks a write knote.
pub fn filt_ptcwdetach(kn: &Knote) {
    let pti = kn_pti(kn);

    let s = spltty();
    klist_remove_locked(&pti.pt_selw.si_note, kn);
    splx(s);
}

/// `filt_ptcwrite`: the master is writable when the slave's input has room.
pub fn filt_ptcwrite(kn: &Knote, _hint: i64) -> bool {
    let pti = kn_pti(kn);
    let tp = pti.pt_tty;
    kn.kn_data().set(0);

    if tp.t_state_isset(TS_ISOPEN) {
        let (rawcc, cancc) = (tp.t_rawq.c_cc.get(), tp.t_canq.c_cc.get());
        if pti.pt_flags.get() & PF_REMOTE != 0 {
            if cancc == 0 {
                kn.kn_data().set(i64::from(tp.t_canq.c_cn.get()));
            }
        } else if rawcc + cancc < ttyhog(tp) - 2 || (cancc == 0 && tp.t_lflag() & ICANON != 0) {
            kn.kn_data()
                .set(i64::from(tp.t_canq.c_cn.get() - (rawcc + cancc)));
        }
    }
    let mut active = kn.kn_data().get() > 0;

    // Write-side HUP condition is only for poll(2) and select(2).
    if kn.has_flags(__EV_POLL | __EV_SELECT) {
        if !tp.t_state_isset(TS_CARR_ON) {
            kn.set_flags(__EV_HUP);
            active = true;
        } else {
            kn.clear_flags(__EV_HUP);
        }
    }

    active
}

/// `filt_ptcexcept`: out-of-band data (packet or user-control mode) and, for poll, the
/// hang-up.
pub fn filt_ptcexcept(kn: &Knote, _hint: i64) -> bool {
    let pti = kn_pti(kn);
    let tp = pti.pt_tty;
    let mut active = false;

    if kn.kn_sfflags.get() & NOTE_OOB != 0 {
        // If in packet or user control mode, check for data.
        if (pti.pt_flags.get() & PF_PKT != 0 && pti.pt_send.get() != 0)
            || (pti.pt_flags.get() & PF_UCNTL != 0 && pti.pt_ucntl.get() != 0)
        {
            kn.kn_fflags().set(kn.kn_fflags().get() | NOTE_OOB);
            kn.kn_data().set(1);
            active = true;
        }
    }

    if kn.has_flags(__EV_POLL) {
        if !tp.t_state_isset(TS_CARR_ON) {
            kn.set_flags(__EV_HUP);
            active = true;
        } else {
            kn.clear_flags(__EV_HUP);
        }
    }

    active
}

/// `ptckqfilter`: attaches a kqueue filter to the master side.
pub fn ptckqfilter(dev: Dev, kn: &Knote) -> Result<(), Errno> {
    let Some(pti) = pt_softc(minor(dev)) else {
        return Err(Errno::ENXIO);
    };

    let klist = match kn.kn_filter().get() {
        EVFILT_READ => {
            kn.kn_fop.set(Some(&PTCREAD_FILTOPS));
            &pti.pt_selr.si_note
        }
        EVFILT_WRITE => {
            kn.kn_fop.set(Some(&PTCWRITE_FILTOPS));
            &pti.pt_selw.si_note
        }
        EVFILT_EXCEPT => {
            kn.kn_fop.set(Some(&PTCEXCEPT_FILTOPS));
            &pti.pt_selr.si_note
        }
        _ => return Err(Errno::EINVAL),
    };

    kn.kn_hook.set(ptr::from_ref(pti).cast_mut().cast());

    let s = spltty();
    klist_insert_locked(klist, kn);
    splx(s);

    Ok(())
}

/// `ptytty` (also `ptstty` and `ptctty`): the tty of a pty.
pub fn ptytty(dev: Dev) -> Option<&'static Tty> {
    pt_softc(minor(dev)).map(|pti| pti.pt_tty)
}

/// `ptyioctl` (also `ptsioctl` and `ptcioctl`): ioctls on either side of a pty.
pub fn ptyioctl(dev: Dev, cmd: u64, data: &mut [u8], flag: i32, p: &Proc) -> Result<(), Errno> {
    let (pti, tp) = pty_of(dev)?;
    let mut cmd = cmd;

    // IF CONTROLLER STTY THEN MUST FLUSH TO PREVENT A HANG. ttywflush(tp) will hang if
    // there are characters in the outq.
    if cmd == TIOCEXT {
        // When the EXTPROC bit is being toggled, we need to send an TIOCPKT_IOCTL if the
        // packet driver is turned on.
        if ioctl_arg::<i32>(data) != 0 {
            if pti.pt_flags.get() & PF_PKT != 0 {
                pti.pt_send.set(pti.pt_send.get() | TIOCPKT_IOCTL as u8);
                ptcwakeup(tp, FREAD);
            }
            tp.t_lflag_set(EXTPROC);
        } else {
            if tp.t_lflag() & EXTPROC != 0 && pti.pt_flags.get() & PF_PKT != 0 {
                pti.pt_send.set(pti.pt_send.get() | TIOCPKT_IOCTL as u8);
                ptcwakeup(tp, FREAD);
            }
            tp.t_lflag_clr(EXTPROC);
        }
        return Ok(());
    } else if ptr::fn_addr_eq(cdevsw(major(dev)).d_open, ptcopen as DevTypeOpen) {
        match cmd {
            TIOCGPGRP => {
                // We avoid calling ttioctl on the controller since, in that case, tp must
                // be the controlling terminal.
                ioctl_ret(data, &tp.pgrp().map_or(0, |pg| pg.pg_id.get()));
                return Ok(());
            }
            TIOCPKT => {
                if ioctl_arg::<i32>(data) != 0 {
                    if pti.pt_flags.get() & PF_UCNTL != 0 {
                        return Err(Errno::EINVAL);
                    }
                    pti.pt_flags.set(pti.pt_flags.get() | PF_PKT);
                } else {
                    pti.pt_flags.set(pti.pt_flags.get() & !PF_PKT);
                }
                return Ok(());
            }
            TIOCUCNTL => {
                if ioctl_arg::<i32>(data) != 0 {
                    if pti.pt_flags.get() & PF_PKT != 0 {
                        return Err(Errno::EINVAL);
                    }
                    pti.pt_flags.set(pti.pt_flags.get() | PF_UCNTL);
                } else {
                    pti.pt_flags.set(pti.pt_flags.get() & !PF_UCNTL);
                }
                return Ok(());
            }
            TIOCREMOTE => {
                if ioctl_arg::<i32>(data) != 0 {
                    pti.pt_flags.set(pti.pt_flags.get() | PF_REMOTE);
                } else {
                    pti.pt_flags.set(pti.pt_flags.get() & !PF_REMOTE);
                }
                ttyflush(tp, FREAD | FWRITE);
                return Ok(());
            }
            TIOCSETD | TIOCSETA | TIOCSETAW | TIOCSETAF => {
                ndflush(&tp.t_outq, tp.t_outq.c_cc.get());
            }
            TIOCSIG => {
                let sig = ioctl_arg::<i32>(data) as u32;
                if sig >= NSIG as u32 || sig == 0 {
                    return Err(Errno::EINVAL);
                }
                if tp.t_lflag() & NOFLSH == 0 {
                    ttyflush(tp, FREAD | FWRITE);
                }
                pgsignal(tp.pgrp(), sig as i32, true);
                if sig as i32 == SIGINFO && tp.t_lflag() & NOKERNINFO == 0 {
                    ttyinfo(tp);
                }
                return Ok(());
            }
            FIONREAD => {
                // FIONREAD on the master side must return the amount in the output queue
                // rather than the input.
                ioctl_ret(data, &tp.t_outq.c_cc.get());
                return Ok(());
            }
            _ => {}
        }
    }
    let mut error = (linesw(tp).l_ioctl)(tp, cmd, data, flag, p);
    if error == Ok(false) {
        error = ttioctl(tp, cmd, data, flag, p);
    }
    let error = match error {
        Ok(true) => Ok(()),
        Err(e) => Err(e),
        Ok(false) => 'notty: {
            // Translate TIOCSBRK/TIOCCBRK to user mode ioctls to let the master interpret
            // BREAK conditions.
            match cmd {
                TIOCSBRK => cmd = uioccmd(TIOCUCNTL_SBRK as u8),
                TIOCCBRK => cmd = uioccmd(TIOCUCNTL_CBRK as u8),
                _ => {}
            }
            if pti.pt_flags.get() & PF_UCNTL != 0 && (cmd & !0xff) == uioccmd(0) {
                if cmd & 0xff != 0 {
                    pti.pt_ucntl.set(cmd as u8);
                    ptcwakeup(tp, FREAD);
                }
                return Ok(());
            }
            break 'notty Err(Errno::ENOTTY);
        }
    };
    // If external processing and packet mode send ioctl packet.
    if tp.t_lflag() & EXTPROC != 0
        && pti.pt_flags.get() & PF_PKT != 0
        && matches!(cmd, TIOCSETA | TIOCSETAW | TIOCSETAF)
    {
        pti.pt_send.set(pti.pt_send.get() | TIOCPKT_IOCTL as u8);
        ptcwakeup(tp, FREAD);
    }
    let stop = tp.t_iflag() & IXON != 0
        && cceq(tp.t_cc(VSTOP), ctrl(b's'))
        && cceq(tp.t_cc(VSTART), ctrl(b'q'));
    if pti.pt_flags.get() & PF_NOSTOP != 0 {
        if stop {
            pti.pt_send
                .set((pti.pt_send.get() & !(TIOCPKT_NOSTOP as u8)) | TIOCPKT_DOSTOP as u8);
            pti.pt_flags.set(pti.pt_flags.get() & !PF_NOSTOP);
            ptcwakeup(tp, FREAD);
        }
    } else if !stop {
        pti.pt_send
            .set((pti.pt_send.get() & !(TIOCPKT_DOSTOP as u8)) | TIOCPKT_NOSTOP as u8);
        pti.pt_flags.set(pti.pt_flags.get() | PF_NOSTOP);
        ptcwakeup(tp, FREAD);
    }
    error
}

/// `sysctl_pty`: return pty-related information (`kern.tty` names `sysctl_tty` does not
/// know).
pub fn sysctl_pty(
    name: &[i32],
    _oldp: usize,
    _oldlenp: &mut usize,
    _newp: usize,
    _newlen: usize,
) -> Result<(), Errno> {
    if name.len() != 1 {
        return Err(Errno::ENOTDIR);
    }

    Err(Errno::EOPNOTSUPP)
}

/// `pty_isfree_locked`: check if a pty is free to use.
fn pty_isfree_locked(minor: u32) -> bool {
    match pt_softc(minor) {
        None => true,
        Some(pt) => pt.pt_tty.t_oproc.get().is_none(),
    }
}

/// `pty_isfree`.
fn pty_isfree(minor: u32) -> bool {
    rw_enter_read(&PT_SOFTC_LOCK);
    let isfree = pty_isfree_locked(minor);
    rw_exit_read(&PT_SOFTC_LOCK);
    isfree
}

/// `pty_getfree`: the first free pty, or the one past the array.
pub fn pty_getfree() -> Dev {
    rw_enter_read(&PT_SOFTC_LOCK);
    let npty = PT.npty.get().max(0) as u32;
    let mut i = 0;
    while i < npty {
        if pty_isfree_locked(i) {
            break;
        }
        i += 1;
    }
    rw_exit_read(&PT_SOFTC_LOCK);
    makedev(PTS_MAJOR.load(Ordering::Relaxed) as u32, i)
}

/// `ptm_vn_open`: hacked up version of `vn_open`. We _only_ handle ptys and only open them
/// with `FREAD|FWRITE` and never deal with creat or stuff like that.
///
/// We need it because we have to fake up root credentials to open the pty.
fn ptm_vn_open(ndp: &mut Nameidata<'_>) -> Result<(), Errno> {
    namei(ndp)?;
    let p = ndp.ni_cnd.proc();
    // SAFETY: the thread outlives its own open; detached from `ndp`'s borrow.
    let p: &Proc = unsafe { &*ptr::from_ref(p) };
    let Some(vp) = ndp.ni_vp else {
        return Err(Errno::ENOENT);
    };
    if vp.v_type.get() != VCHR {
        vput(vp);
        return Err(Errno::EINVAL);
    }

    // Get us a fresh cred with root privileges.
    let cred = crget();
    let error = VOP_OPEN(vp, FREAD | FWRITE, cred, p);
    if error.is_ok() {
        // update atime/mtime
        let mut vattr = Vattr::new();
        vattr_null(&mut vattr);
        vattr.va_atime = getnanotime();
        vattr.va_mtime = vattr.va_atime;
        vattr.va_vaflags |= VA_UTIMES_NULL;
        let _ = VOP_SETATTR(vp, &mut vattr, p.p_ucred.get(), p);
    }
    crfree(cred);

    if let Err(e) = error {
        vput(vp);
        return Err(e);
    }

    vp.v_writecount.set(vp.v_writecount.get() + 1);

    Ok(())
}

/// `ptmattach`: find the major and minor of the pty devices.
pub fn ptmattach(_n: i32) {
    let n = nchrdev();
    let mut i = 0;
    while i < n {
        if ptr::fn_addr_eq(cdevsw(i).d_open, ptsopen as DevTypeOpen) {
            break;
        }
        i += 1;
    }

    if i == n {
        panic(format_args!("ptmattach: Can't find pty slave in cdevsw"));
    }

    PTS_MAJOR.store(i as i32, Ordering::Relaxed);
}

/// `ptmopen`.
pub fn ptmopen(_dev: Dev, _flag: i32, _mode: i32, _p: &Proc) -> Result<(), Errno> {
    Ok(())
}

/// `ptmclose`.
pub fn ptmclose(_dev: Dev, _flag: i32, _mode: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    Ok(())
}

/// `ptmioctl`: `PTMGET` hands out a free master/slave pair, opened, at two new descriptors.
pub fn ptmioctl(_dev: Dev, cmd: u64, data: &mut [u8], _flag: i32, p: &Proc) -> Result<(), Errno> {
    let fdp = p.fd();

    if cmd != PTMGET {
        return Err(Errno::EINVAL);
    }

    fdplock(fdp);
    // Grab two filedescriptors.
    let (cfp, cindx) = match falloc(p) {
        Ok(r) => r,
        Err(e) => {
            fdpunlock(fdp);
            return Err(e);
        }
    };
    let (sfp, sindx) = match falloc(p) {
        Ok(r) => r,
        Err(e) => {
            fdremove(fdp, cindx);
            fdpunlock(fdp);
            let _ = closef(cfp, p);
            return Err(e);
        }
    };
    fdpunlock(fdp);

    let result = 'bad: loop {
        // retry:
        // Find and open a free master pty.
        let newdev = pty_getfree();
        if let Err(e) = check_pty(newdev) {
            break 'bad Err(e);
        }
        let Some(pti) = pt_softc(minor(newdev)) else {
            break 'bad Err(Errno::ENXIO);
        };
        let pn = pti.pty_pn.get();
        let mut cnd = ndinit(
            LOOKUP,
            NOFOLLOW | LOCKLEAF | KERNELPATH,
            NiDirp::Sys(&pn),
            p,
        );
        cnd.ni_pledge = PLEDGE_RPATH | PLEDGE_WPATH;
        if let Err(e) = ptm_vn_open(&mut cnd) {
            // Check if the master open failed because we lost the race to grab it.
            if e == Errno::EIO && !pty_isfree(minor(newdev)) {
                continue 'bad;
            }
            break 'bad Err(e);
        }
        let Some(cvp) = cnd.ni_vp else {
            break 'bad Err(Errno::ENOENT);
        };
        cfp.f_flag.store((FREAD | FWRITE) as u32, Ordering::SeqCst);
        cfp.f_type.set(DTYPE_VNODE);
        cfp.f_ops.set(Some(&VNOPS));
        cfp.f_data
            .set(ptr::from_ref(cvp).cast_mut().cast::<c_void>());
        let _ = VOP_UNLOCK(cvp);

        // Open the slave.
        // namei -> setattr -> unlock -> revoke -> vrele -> namei -> open -> unlock
        // Three stage rocket:
        // 1. Change the owner and permissions on the slave.
        // 2. Revoke all the users of the slave.
        // 3. open the slave.
        let sn = pti.pty_sn.get();
        let mut snd = ndinit(
            LOOKUP,
            NOFOLLOW | LOCKLEAF | KERNELPATH,
            NiDirp::Sys(&sn),
            p,
        );
        snd.ni_pledge = PLEDGE_RPATH | PLEDGE_WPATH;
        if let Err(e) = namei(&mut snd) {
            break 'bad Err(e);
        }
        let Some(svp) = snd.ni_vp else {
            break 'bad Err(Errno::ENOENT);
        };
        if svp
            .v_mount
            .get()
            .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY == 0)
        {
            let gid = TTY_GID_;
            // get real uid
            let uid = p.ucred().cr_ruid.get();

            let mut vattr = Vattr::new();
            vattr_null(&mut vattr);
            vattr.va_uid = uid;
            vattr.va_gid = gid;
            vattr.va_mode = (S_IRUSR | S_IWUSR | S_IWGRP) & ALLPERMS;
            // Get a fake cred to pretend we're root.
            let cred = crget();
            let error = VOP_SETATTR(svp, &mut vattr, cred, p);
            crfree(cred);
            if let Err(e) = error {
                vput(svp);
                break 'bad Err(e);
            }
        }
        let _ = VOP_UNLOCK(svp);
        if svp.v_usecount.get() > 1 || svp.v_flag.get() & VALIASED != 0 {
            let _ = VOP_REVOKE(svp, REVOKEALL);
        }

        // The vnode is useless after the revoke, we need to namei again.
        vrele(svp);

        let mut snd = ndinit(
            LOOKUP,
            NOFOLLOW | LOCKLEAF | KERNELPATH,
            NiDirp::Sys(&sn),
            p,
        );
        snd.ni_pledge = PLEDGE_RPATH | PLEDGE_WPATH;
        // now open it
        if let Err(e) = ptm_vn_open(&mut snd) {
            break 'bad Err(e);
        }
        let Some(svp) = snd.ni_vp else {
            break 'bad Err(Errno::ENOENT);
        };
        sfp.f_flag.store((FREAD | FWRITE) as u32, Ordering::SeqCst);
        sfp.f_type.set(DTYPE_VNODE);
        sfp.f_ops.set(Some(&VNOPS));
        sfp.f_data
            .set(ptr::from_ref(svp).cast_mut().cast::<c_void>());
        let _ = VOP_UNLOCK(svp);

        // now, put the indexen and names into struct ptmget
        let mut ptm = Ptmget {
            cfd: cindx,
            sfd: sindx,
            ..Ptmget::default()
        };
        ptm.cn[..pn.len()].copy_from_slice(&pn);
        ptm.sn[..sn.len()].copy_from_slice(&sn);
        ioctl_ret(data, &ptm);

        // insert files now that we've passed all errors
        fdplock(fdp);
        fdinsert(fdp, cindx, 0, cfp);
        fdinsert(fdp, sindx, 0, sfp);
        fdpunlock(fdp);
        let _ = frele(cfp, p);
        let _ = frele(sfp, p);
        break 'bad Ok(());
    };

    if result.is_err() {
        // bad:
        fdplock(fdp);
        fdremove(fdp, cindx);
        fdremove(fdp, sindx);
        fdpunlock(fdp);
        let _ = closef(cfp, p);
        let _ = closef(sfp, p);
    }
    result
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the ptys: a master and its slave passing a line each way through the line
    // discipline, and the device switch finding the slave's major.

    use super::*;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::machine::conf::cdevsw;
    use crate::sys::conf::D_TTY;
    use crate::sys::proc::{Pgrp, Process};
    use crate::sys::uio::{Iovec, UioRw, UioSeg};
    use std::boxed::Box;
    use std::vec;
    use std::vec::Vec;

    /// A thread in a process with a group (what the entry points read through `p`).
    struct World {
        pr: Process,
        p: Proc,
        pg: Pgrp,
    }

    fn world() -> Box<World> {
        let w = Box::new(World {
            pr: Process::new(),
            p: Proc::new(),
            pg: Pgrp::new(),
        });
        w.pr.ps_pgrp.set(&w.pg);
        w.p.p_p.set(&w.pr);
        w
    }

    /// Moves `buf` through `f` as a kernel-space uio in direction `rw`; the bytes moved.
    fn xfer(
        f: fn(Dev, &mut Uio<'_>, i32) -> Result<(), Errno>,
        dev: Dev,
        rw: UioRw,
        buf: &mut [u8],
    ) -> (Result<(), Errno>, usize) {
        let n = buf.len();
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: n,
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: n,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: rw,
            uio_procp: None,
        };
        let r = f(dev, &mut uio, IO_NDELAY);
        (r, n - uio.uio_resid)
    }

    fn read(f: fn(Dev, &mut Uio<'_>, i32) -> Result<(), Errno>, dev: Dev) -> Vec<u8> {
        let mut buf = vec![0u8; 64];
        let (r, n) = xfer(f, dev, UioRw::UIO_READ, &mut buf);
        assert_eq!(r, Ok(()));
        buf.truncate(n);
        buf
    }

    #[test]
    fn a_line_each_way_between_master_and_slave() {
        let _g = setup_real_memory();
        let w = world();

        // The host's switch has amd64's slots: pts at 5, ptc at 6, ptm at 81.
        assert_eq!(cdevsw(5).d_type, D_TTY);
        assert!(ptr::fn_addr_eq(cdevsw(6).d_open, ptcopen as DevTypeOpen));
        ptyattach(1);
        assert_eq!(PTS_MAJOR.load(Ordering::Relaxed), 5);
        assert_eq!(pty_getfree(), makedev(5, 0));

        let master = makedev(6, 3);
        let slave = makedev(5, 3);
        assert_eq!(ptcopen(master, FREAD | FWRITE, 0, &w.p), Ok(()));
        assert_eq!(
            ptcopen(master, FREAD | FWRITE, 0, &w.p),
            Err(Errno::EIO),
            "busy"
        );
        assert_eq!(ptsopen(slave, FREAD | FWRITE, 0, &w.p), Ok(()));
        let pti = pt_softc(3).expect("check_pty made the slot");
        assert_eq!(&pti.pty_pn.get(), b"/dev/ptyp3\0");
        assert_eq!(&pti.pty_sn.get(), b"/dev/ttyp3\0");

        // Typed on the master: the slave reads the line, the master reads the echo.
        let mut line = *b"hello\n";
        assert_eq!(
            xfer(ptcwrite, master, UioRw::UIO_WRITE, &mut line),
            (Ok(()), 6)
        );
        assert_eq!(read(ptsread, slave), b"hello\n");
        assert_eq!(read(ptcread, master), b"hello\r\n");

        // Written by the slave: output processing, then the master reads it.
        let mut out = *b"out\n";
        assert_eq!(
            xfer(ptswrite, slave, UioRw::UIO_WRITE, &mut out),
            (Ok(()), 4)
        );
        assert_eq!(read(ptcread, master), b"out\r\n");
        let mut buf = [0u8; 8];
        assert_eq!(
            xfer(ptcread, master, UioRw::UIO_READ, &mut buf).0,
            Err(Errno::EWOULDBLOCK)
        );

        // The slave is a tty: TIOCGETA answers.
        let mut t = [0u8; size_of::<Termios>()];
        assert_eq!(
            ptyioctl(slave, crate::sys::ttycom::TIOCGETA, &mut t, 0, &w.p),
            Ok(())
        );

        assert_eq!(ptsclose(slave, FREAD | FWRITE, 0, Some(&w.p)), Ok(()));
        assert_eq!(ptcclose(master, FREAD | FWRITE, 0, Some(&w.p)), Ok(()));
        assert!(pty_isfree(3));
    }
}
/* </TESTS> */
