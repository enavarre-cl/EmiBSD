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
/* </LICENSES> */

/* <CODE> */
//! The console as an open file: a stand-in, not OpenBSD code, until `/dev/console` exists.
//!
//! In OpenBSD the first process gets its descriptors 0, 1 and 2 from `init(8)`, which opens
//! `/dev/console` (`setctty`, `login_tty`): a vnode of the console character device, whose
//! `cdevsw` entry (`cnopen`, `cnread`, `cnwrite`, `cnioctl` in `dev/cons.c`) forwards to the
//! tty of the console driver. The device switch and the tty layer are here, but there is no
//! root file system to hold a `/dev/console` node, so `start_init` calls [`consfile_attach`]
//! instead: one `struct file` of type [`DTYPE_CONSFILE`] installed at descriptors 0, 1 and 2
//! of process 1 through the real descriptor table (`falloc`, `fdinsert`, `fdalloc`), whose
//! `fileops` are what `vn_read`, `vn_write`, `vn_ioctl` and `vn_close` would do for that
//! vnode: they call the console device's entry points (`cnopen`, `cnread`, `cnwrite`,
//! `cnioctl`, `cnclose`), so the console's line discipline applies (echo, erase and kill,
//! `^C` to the foreground process group, `ONLCR`) and the descriptors are a tty
//! (`TIOCGETA`, `TIOCSCTTY`).
//!
//! What the stand-in does, compared with a `/dev/console` vnode:
//! - read/write: `cnread`/`cnwrite` with `IO_NDELAY` when the file is non-blocking, as
//!   `vn_read`/`vn_write` pass it; no offset (a character device has none).
//! - ioctl: `FIONBIO` and `FIOASYNC` are `vn_ioctl`'s for any file; the rest goes to
//!   `cnioctl`. `TIOCSCTTY` makes the console the controlling terminal as `ttioctl` does,
//!   but no vnode is recorded in the session (`s_ttyvp`), so `/dev/tty` cannot reach it.
//! - stat: a character device, mode `0600`, `st_rdev` the console's device number.
//! - close: `cnclose` on the last reference.
//! - kqueue: `cnkqfilter`, the console tty's `ttkqfilter`.
//!
//! When the console device cannot be opened (no console attached as a tty), the stand-in
//! falls back to the polled console it was before the tty layer: writes go to `cnputc`, a
//! read is a line from `cngetc` with echo, and the termios ioctls answer `ENOTTY`.
//!
//! Each operation runs under the kernel lock, as the `vn_*` operations of a `/dev/console`
//! vnode do (M11e: read(2) and write(2) are `SY_NOLOCK`; without it two processes writing
//! to the console interleave their characters).
//!
//! It goes away when `init` can open `/dev/console`.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::dev::cons::{
    CONSMAJOR, cn_tab, cnclose, cngetc, cnioctl, cnkqfilter, cnopen, cnpollc, cnputc, cnread,
    cnwrite,
};
use crate::kern::kern_descrip::{falloc, fdalloc, fdexpand, fdinsert};
use crate::kern::kern_subr::{uiomove, ureadc};
use crate::sys::errno::Errno;
use crate::sys::event::Knote;
use crate::sys::fcntl::{FNONBLOCK, FREAD, FWRITE};
use crate::sys::file::{File, Fileops, fref, frele};
use crate::sys::filedesc::{fdplock, fdpunlock};
use crate::sys::filio::{FIOASYNC, FIONBIO};
use crate::sys::proc::Proc;
use crate::sys::stat::{S_IFCHR, S_IRUSR, S_IWUSR, Stat};
use crate::sys::systm::{kernel_lock, kernel_unlock};
use crate::sys::types::{Dev, makedev};
use crate::sys::uio::Uio;
use crate::sys::vnode::IO_NDELAY;

/// The descriptor type of the console stand-in: outside OpenBSD's `DTYPE_*` range, so no
/// code that switches on a real type mistakes it for one.
pub const DTYPE_CONSFILE: i32 = 127;

/// How much a polled write moves from user space at a time.
const CONSFILE_CHUNK: usize = 256;

/// `/dev/console`'s device number (`makedev(CONSMAJOR, 0)`).
const CONSDEV: Dev = makedev(CONSMAJOR, 0);

/// Whether `cnopen` succeeded: the stand-in goes through the console's tty.
static CONSFILE_TTY: AtomicBool = AtomicBool::new(false);

/// The stand-in's operations.
static CONSFILEOPS: Fileops = Fileops {
    fo_read: consfile_read,
    fo_write: consfile_write,
    fo_ioctl: consfile_ioctl,
    fo_kqfilter: consfile_kqfilter,
    fo_stat: consfile_stat,
    fo_close: consfile_close,
    fo_seek: None,
};

/// `IO_NDELAY` for a non-blocking file, as `vn_read`/`vn_write` compute it.
fn ioflag(fp: &File) -> i32 {
    if fp.flag() & FNONBLOCK != 0 {
        IO_NDELAY
    } else {
        0
    }
}

/// Runs `f` under the kernel lock, as the `vn_*` file operations the stand-in replaces run
/// the vnode's (`vn_read`, `vn_write`, `vn_ioctl`, `vn_kqfilter`, `vn_closefile`).
fn vn_locked<T>(f: impl FnOnce() -> T) -> T {
    kernel_lock();
    let r = f();
    kernel_unlock();
    r
}

/// Reads from the console's tty; without one, a polled line with echo.
fn consfile_read(fp: &File, uio: &mut Uio<'_>, fflags: i32) -> Result<(), Errno> {
    vn_locked(|| consfile_read_locked(fp, uio, fflags))
}

/// [`consfile_read`] under the kernel lock.
fn consfile_read_locked(fp: &File, uio: &mut Uio<'_>, _fflags: i32) -> Result<(), Errno> {
    if CONSFILE_TTY.load(Ordering::Relaxed) {
        return cnread(CONSDEV, uio, ioflag(fp));
    }
    if cn_tab().is_none() {
        return Ok(()); // no console: end of file
    }
    cnpollc(true);
    let mut error = Ok(());
    while uio.uio_resid > 0 {
        let mut c = cngetc();
        if c == i32::from(b'\r') {
            c = i32::from(b'\n');
        }
        cnputc(c);
        if let Err(e) = ureadc(c, uio) {
            error = Err(e);
            break;
        }
        if c == i32::from(b'\n') {
            break;
        }
    }
    cnpollc(false);
    error
}

/// Writes to the console's tty; without one, every byte to `cnputc`.
fn consfile_write(fp: &File, uio: &mut Uio<'_>, fflags: i32) -> Result<(), Errno> {
    vn_locked(|| consfile_write_locked(fp, uio, fflags))
}

/// [`consfile_write`] under the kernel lock.
fn consfile_write_locked(fp: &File, uio: &mut Uio<'_>, _fflags: i32) -> Result<(), Errno> {
    if CONSFILE_TTY.load(Ordering::Relaxed) {
        return cnwrite(CONSDEV, uio, ioflag(fp));
    }
    let mut chunk = [0u8; CONSFILE_CHUNK];
    while uio.uio_resid > 0 {
        let n = uio.uio_resid.min(CONSFILE_CHUNK);
        uiomove(&mut chunk[..n], uio)?;
        for &c in &chunk[..n] {
            cnputc(i32::from(c));
        }
    }
    Ok(())
}

/// `FIONBIO` and `FIOASYNC` are the file's; the rest is the console device's.
fn consfile_ioctl(fp: &File, com: u64, data: &mut [u8], p: &Proc) -> Result<(), Errno> {
    match com {
        FIONBIO | FIOASYNC => Ok(()),
        _ if CONSFILE_TTY.load(Ordering::Relaxed) => {
            vn_locked(|| cnioctl(CONSDEV, com, data, fp.flag(), p))
        }
        _ => Err(Errno::ENOTTY),
    }
}

/// The console device's kqueue filter.
fn consfile_kqfilter(_fp: &File, kn: &Knote) -> Result<(), Errno> {
    vn_locked(|| cnkqfilter(CONSDEV, kn))
}

/// A character device owned by root, mode 0600.
fn consfile_stat(_fp: &File, ub: &mut Stat, _p: &Proc) -> Result<(), Errno> {
    *ub = Stat {
        st_mode: S_IFCHR | S_IRUSR | S_IWUSR,
        st_rdev: cn_tab().map_or(0, |cp| cp.cn_dev.get()),
        st_nlink: 1,
        ..Stat::default()
    };
    Ok(())
}

/// The last reference closes the console device.
fn consfile_close(fp: &File, p: Option<&Proc>) -> Result<(), Errno> {
    if CONSFILE_TTY.swap(false, Ordering::Relaxed) {
        return vn_locked(|| cnclose(CONSDEV, fp.flag(), S_IFCHR as i32, p));
    }
    Ok(())
}

/// Installs the console stand-in at descriptors 0, 1 and 2 of `p`'s (empty) table: one file
/// open for reading and writing, as `init(8)`'s `open(_PATH_CONSOLE, O_RDWR)` and
/// `login_tty`'s `dup2`s would leave it, with the console device opened (`cnopen`).
pub fn consfile_attach(p: &Proc) -> Result<(), Errno> {
    let fdp = p.fd();

    let opened = cnopen(CONSDEV, FREAD | FWRITE, S_IFCHR as i32, p).is_ok()
        && cn_tab().is_some_and(|cp| cp.cn_dev.get() != crate::sys::param::NODEV);
    CONSFILE_TTY.store(opened, Ordering::Relaxed);

    fdplock(fdp);
    let (fp, fd) = match falloc(p) {
        Ok(r) => r,
        Err(e) => {
            fdpunlock(fdp);
            return Err(e);
        }
    };
    fp.f_flag.store((FREAD | FWRITE) as u32, Ordering::SeqCst);
    fp.f_type.set(DTYPE_CONSFILE);
    fp.f_ops.set(Some(&CONSFILEOPS));
    fdinsert(fdp, fd, 0, fp);

    let mut error = Ok(());
    for want in fd + 1..=2 {
        let new = loop {
            match fdalloc(p, want) {
                Err(Errno::ENOSPC) => {
                    if let Err(e) = fdexpand(p) {
                        break Err(e);
                    }
                }
                r => break r,
            }
        };
        match new {
            Ok(new) => {
                fref(fp);
                fdinsert(fdp, new, 0, fp);
            }
            Err(e) => {
                error = Err(e);
                break;
            }
        }
    }
    fdpunlock(fdp);

    let _ = frele(fp, p);
    error
}
/* </CODE> */
